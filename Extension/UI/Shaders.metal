#include <metal_stdlib>
using namespace metal;

// Must match PointVertex in ParticleSystem.swift.
struct PointVertex {
    float2 position;
    float size;
    float pad;
    float4 color;
};

// Must match CompositeUniforms in ScopeRenderer.swift.
struct CompositeUniforms {
    float2 focus;
    float focusAmount;
    float bloomStrength;
    float2 resolution;
    float margin;
    float exposure;
};

struct PointOut {
    float4 position [[position]];
    float size [[point_size]];
    float4 color;
};

struct LineOut {
    float4 position [[position]];
    float4 color;
};

struct QuadOut {
    float4 position [[position]];
    float2 uv;
};

constexpr sampler linearClamp(filter::linear, address::clamp_to_edge);

// ---- Geometry (additive into the HDR persistence buffer) -------------------------------------

vertex PointOut point_vertex(const device PointVertex* v [[buffer(0)]], uint id [[vertex_id]]) {
    PointOut o;
    o.position = float4(v[id].position, 0, 1);
    o.size = v[id].size;
    o.color = v[id].color;
    return o;
}

fragment float4 point_fragment(PointOut in [[stage_in]], float2 pc [[point_coord]]) {
    const float2 d = pc * 2.0 - 1.0;
    const float r2 = dot(d, d);
    if (r2 > 1.0) discard_fragment();
    const float core = exp(-r2 * 9.0);
    const float halo = exp(-r2 * 2.5) * 0.35;
    return float4(in.color.rgb * (core + halo), 1);
}

vertex LineOut line_vertex(const device PointVertex* v [[buffer(0)]], uint id [[vertex_id]]) {
    LineOut o;
    o.position = float4(v[id].position, 0, 1);
    o.color = v[id].color;
    return o;
}

fragment float4 line_fragment(LineOut in [[stage_in]]) { return float4(in.color.rgb, 1); }

// ---- Full-screen passes ------------------------------------------------------------------------

vertex QuadOut fullscreen_vertex(uint id [[vertex_id]]) {
    const float2 p = float2((id << 1) & 2, id & 2);
    QuadOut o;
    o.position = float4(p * 2.0 - 1.0, 0, 1);
    o.uv = float2(p.x, 1.0 - p.y);
    return o;
}

// Phosphor decay: blended as dst * src.
fragment float4 fade_fragment(QuadOut in [[stage_in]], constant float& decay [[buffer(0)]]) {
    return float4(decay);
}

fragment float4 downsample_fragment(QuadOut in [[stage_in]], texture2d<float> src [[texture(0)]]) {
    const float3 c = src.sample(linearClamp, in.uv).rgb;
    return float4(max(c - 0.04, 0.0), 1);
}

fragment float4 blur_fragment(QuadOut in [[stage_in]],
                              texture2d<float> src [[texture(0)]],
                              constant float2& step [[buffer(0)]]) {
    const float w[5] = {0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216};
    float3 c = src.sample(linearClamp, in.uv).rgb * w[0];
    for (int i = 1; i < 5; ++i) {
        c += src.sample(linearClamp, in.uv + step * float(i)).rgb * w[i];
        c += src.sample(linearClamp, in.uv - step * float(i)).rgb * w[i];
    }
    return float4(c, 1);
}

static float lineMask(float distancePixels, float halfWidth) {
    return 1.0 - clamp(distancePixels - halfWidth, 0.0, 1.0);
}

fragment float4 composite_fragment(QuadOut in [[stage_in]],
                                   texture2d<float> accum [[texture(0)]],
                                   texture2d<float> bloom [[texture(1)]],
                                   constant CompositeUniforms& u [[buffer(0)]]) {
    float3 c = accum.sample(linearClamp, in.uv).rgb + bloom.sample(linearClamp, in.uv).rgb * u.bloomStrength;

    const float2 p = float2(in.uv.x, 1.0 - in.uv.y);  // y-up view space
    const float2 q = (p - u.margin) / (1.0 - 2.0 * u.margin);  // plot space
    const float2 plotPixels = u.resolution * (1.0 - 2.0 * u.margin);

    // Graticule: 10×8 divisions, centre axes with minor ticks, and a frame.
    float3 grid = float3(0);
    if (all(q >= -0.002) && all(q <= 1.002)) {
        const float2 g = q * float2(10.0, 8.0);
        const float2 gd = abs(fract(g + 0.5) - 0.5) * plotPixels / float2(10.0, 8.0);
        const float minor = max(lineMask(gd.x, 0.0), lineMask(gd.y, 0.0));
        const float2 ad = abs(q - 0.5) * plotPixels;
        const float axis = max(lineMask(ad.x, 0.25), lineMask(ad.y, 0.25));
        const float2 t = q * float2(50.0, 40.0);
        const float2 td = abs(fract(t + 0.5) - 0.5) * plotPixels / float2(50.0, 40.0);
        const float ticks = lineMask(td.x, 0.0) * step(ad.y, 4.0) + lineMask(td.y, 0.0) * step(ad.x, 4.0);
        const float2 ed = min(q, 1.0 - q) * plotPixels;
        const float frame = lineMask(min(abs(ed.x), abs(ed.y)), 0.5);
        grid = float3(0.16, 0.5, 0.44) * (minor * 0.09 + axis * 0.16 + ticks * 0.22 + frame * 0.22);
    }

    // Focus crosshair (amber), with a ring that grows with the Focus amount.
    const float2 fd = (q - u.focus) * plotPixels;
    const float cross = (lineMask(abs(fd.y), 0.25) * step(abs(fd.x), 9.0) +
                         lineMask(abs(fd.x), 0.25) * step(abs(fd.y), 9.0)) * step(3.0, length(fd));
    const float radius = 14.0 + u.focusAmount * 90.0;
    const float ring = lineMask(abs(length(fd) - radius), 0.25) * (0.15 + 0.6 * u.focusAmount);
    const float3 amber = float3(1.0, 0.62, 0.22);

    const float vignette = smoothstep(1.05, 0.35, length(p - 0.5) * 1.35);
    c = 1.0 - exp(-c * u.exposure * vignette);
    c += grid + amber * (cross * 0.45 + ring * 0.45);
    c += float3(0.006, 0.014, 0.016);
    return float4(c, 1);
}
