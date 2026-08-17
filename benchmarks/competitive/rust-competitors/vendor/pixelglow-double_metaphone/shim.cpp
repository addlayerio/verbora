// A thin C-ABI wrapper around the vendored double_metaphone.h, so
// benches/double_metaphone_cpp.rs can call it through Rust FFI. This file
// is this workspace's own code, not part of the vendored library -- see
// README.md for the library's provenance.

#include "double_metaphone.h"
#include <cstring>
#include <string>

extern "C" {

// Writes dm::double_metaphone(input)'s primary/secondary keys into
// caller-owned, null-terminated buffers. Each `*_cap` is the buffer size
// including the terminating byte; a key longer than `cap - 1` is
// truncated, never overflowed. In practice this never truncates for real
// benchmark input -- double metaphone keys stay a small multiple of the
// input length, and BUF_CAP on the Rust side (256 bytes) is far beyond
// any realistic word or name.
void dm_double_metaphone(
    const char* input,
    char* primary_out, size_t primary_cap,
    char* secondary_out, size_t secondary_cap)
{
    auto result = dm::double_metaphone(std::string(input));

    auto copy_bounded = [](char* out, size_t cap, const std::string& s)
    {
        if (cap == 0)
            return;
        size_t len = s.size() < cap - 1 ? s.size() : cap - 1;
        std::memcpy(out, s.data(), len);
        out[len] = '\0';
    };

    copy_bounded(primary_out, primary_cap, result.first);
    copy_bounded(secondary_out, secondary_cap, result.second);
}

}
