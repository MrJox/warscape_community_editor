#pragma once
// CA compiled shader container (data.pack/fxc/*.fxc, DX11 variants).
//
//   u32 magic (0x075BCD15), u32 version (4), u32 define_count
//   define_count x { u32 len, char name[len] (NUL incl.), u32 len, char value[len] }
//   u32 source_timestamp, u32 include_count
//   include_count x { u16 len, char path[len], u32 hash }
//   DXBC blob to end of file (has RDEF reflection)
//
// File names: <defines>_<sm>_<source>.hlsl_<stage>_<entry>.fxc, e.g.
//   dx11_sm_5_0_rigidstandard.hlsl_ps_ps30_gbuffer.fxc
// (verified on all 3801 dx11 files in data.pack, 2026-10-06)
#include <cstdint>
#include <string>
#include <utility>
#include <vector>

namespace we {

struct FxcFile {
    std::vector<std::pair<std::string, std::string>> defines;
    uint32_t source_timestamp = 0;
    std::vector<std::pair<std::string, uint32_t>> includes;
    size_t dxbc_offset = 0;
    std::vector<uint8_t> bytes; // whole file; DXBC = bytes[dxbc_offset..]

    const uint8_t* dxbc() const { return bytes.data() + dxbc_offset; }
    size_t dxbc_size() const { return bytes.size() - dxbc_offset; }
};

FxcFile parse_fxc(std::vector<uint8_t> bytes); // throws std::runtime_error

// Split "dx11_sm_5_0_rigidstandard.hlsl_ps_ps30_gbuffer.fxc" into its parts.
struct FxcName {
    std::string prefix;  // "dx11_enable_shadows_..._sm_5_0" (api + defines + shader model)
    std::string source;  // "rigidstandard.hlsl"
    std::string stage;   // "vs", "ps", "gs", "cs", ...
    std::string entry;   // "ps30_gbuffer"
};
bool split_fxc_name(const std::string& file_name, FxcName& out);

} // namespace we
