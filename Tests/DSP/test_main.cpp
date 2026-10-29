// Runs every registered TEST_CASE. Pass a substring to run a subset: ./dsp_tests engine
#include "TestHarness.hpp"

int main(int argc, char** argv) {
    const char* filter = argc > 1 ? argv[1] : nullptr;
    int run = 0, failed = 0;
    for (const auto& c : test::registry()) {
        if (filter && !std::strstr(c.name, filter)) continue;
        ++run;
        test::caseFailures() = 0;
        try {
            c.fn();
        } catch (const test::Abort&) {
        } catch (const std::exception& e) {
            test::fail(__FILE__, __LINE__, e.what());
        }
        const bool ok = test::caseFailures() == 0;
        failed += ok ? 0 : 1;
        std::printf("%s %s\n", ok ? "  ok  " : "  FAIL", c.name);
    }
    std::printf("\n%d tests, %d failed\n", run, failed);
    return failed == 0 ? 0 : 1;
}
