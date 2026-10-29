// Lock-free single-producer / single-consumer "latest value" exchange.
#pragma once

#include <atomic>

namespace sr {

template <typename T>
class TripleBuffer {
public:
    // Producer: fill backBuffer(), then publish().
    T& backBuffer() { return buffers_[back_]; }
    void publish() { back_ = middle_.exchange(back_ | kDirty, std::memory_order_acq_rel) & kIndex; }

    // Consumer: returns the most recently published value.
    const T& read() {
        if (middle_.load(std::memory_order_acquire) & kDirty)
            front_ = middle_.exchange(front_, std::memory_order_acq_rel) & kIndex;
        return buffers_[front_];
    }

private:
    static constexpr int kIndex = 3;
    static constexpr int kDirty = 4;
    T buffers_[3] {};
    std::atomic<int> middle_ {1};
    int back_ = 0;
    int front_ = 2;
};

} // namespace sr
