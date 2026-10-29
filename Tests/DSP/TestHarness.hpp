// Minimal dependency-free test harness for the C++ DSP core.
#pragma once

#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <vector>

namespace test {

struct Case {
    const char* name;
    void (*fn)();
};

inline std::vector<Case>& registry() {
    static std::vector<Case> cases;
    return cases;
}

struct Registrar {
    Registrar(const char* name, void (*fn)()) { registry().push_back({name, fn}); }
};

struct Abort {};

inline int& caseFailures() {
    static int n = 0;
    return n;
}

inline void fail(const char* file, int line, const char* message) {
    std::printf("    %s:%d: %s\n", file, line, message);
    ++caseFailures();
}

} // namespace test

#define TEST_CASE(name)                                       \
    static void name();                                       \
    static test::Registrar name##_registrar(#name, name);     \
    static void name()

#define CHECK(cond)                                           \
    do {                                                      \
        if (!(cond)) test::fail(__FILE__, __LINE__, #cond);   \
    } while (0)

#define REQUIRE(cond)                                         \
    do {                                                      \
        if (!(cond)) {                                        \
            test::fail(__FILE__, __LINE__, #cond);            \
            throw test::Abort {};                             \
        }                                                     \
    } while (0)

#define CHECK_NEAR(actual, expected, tolerance)                                                  \
    do {                                                                                         \
        const double a_ = (actual), e_ = (expected), t_ = (tolerance);                           \
        if (!(std::fabs(a_ - e_) <= t_)) {                                                       \
            char msg_[256];                                                                      \
            std::snprintf(msg_, sizeof msg_, "%s = %g, expected %g ± %g", #actual, a_, e_, t_);  \
            test::fail(__FILE__, __LINE__, msg_);                                                \
        }                                                                                        \
    } while (0)

#define CHECK_RANGE(actual, lo, hi)                                                              \
    do {                                                                                         \
        const double a_ = (actual), l_ = (lo), h_ = (hi);                                        \
        if (!(a_ >= l_ && a_ <= h_)) {                                                           \
            char msg_[256];                                                                      \
            std::snprintf(msg_, sizeof msg_, "%s = %g, expected in [%g, %g]", #actual, a_, l_, h_); \
            test::fail(__FILE__, __LINE__, msg_);                                                \
        }                                                                                        \
    } while (0)
