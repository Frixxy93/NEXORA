// =============================================================================
// NEXORA Bridge — compiled Maya plug-in (.mll)  ·  spec §32-§39
// -----------------------------------------------------------------------------
// A real C++ Maya plug-in (like FrixxyMatLib.mll) that connects Maya to the
// NEXORA desktop app over its localhost Bridge API.
//
//   * Registers commands:  nexoraLibrary, nexoraApply, nexoraCapture, nexoraSync
//   * Runs an MTimerMessage poller: heartbeats NEXORA ("Maya connected") and
//     drains the desktop "Send to Maya" queue, applying materials/textures.
//   * Reads host/port/token from ~/.nexora/bridge.json (written by the desktop).
//
// HTTP + JSON are handled in C++; scene/shader work is done by executing MEL
// (the standard, robust division — the shader logic mirrors the verified Python
// plug-in). Never touches NEXORA's database directly; all traffic is localhost.
//
// Build: see CMakeLists.txt + README.md (needs the Maya devkit + a C++ compiler,
// e.g. Visual Studio on Windows).  Author: FRIXXY · MIT
// =============================================================================

#include <maya/MFnPlugin.h>
#include <maya/MPxCommand.h>
#include <maya/MArgList.h>
#include <maya/MArgDatabase.h>
#include <maya/MSyntax.h>
#include <maya/MGlobal.h>
#include <maya/MString.h>
#include <maya/MStringArray.h>
#include <maya/MTimerMessage.h>
#include <maya/MMessage.h>
#include <maya/MStatus.h>

#include <string>
#include <vector>
#include <cstdlib>
#include <cstdio>
#include <cstring>

#include <nlohmann/json.hpp>
using json = nlohmann::json;

#ifdef _WIN32
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #pragma comment(lib, "ws2_32.lib")
  #define NEXORA_EXPORT __declspec(dllexport)
  typedef SOCKET nexora_socket_t;
  #define NEXORA_CLOSESOCK closesocket
#else
  #include <sys/socket.h>
  #include <netinet/in.h>
  #include <arpa/inet.h>
  #include <unistd.h>
  #define NEXORA_EXPORT
  typedef int nexora_socket_t;
  #define INVALID_SOCKET (-1)
  #define NEXORA_CLOSESOCK ::close
#endif

static const char* kVendor  = "FRIXXY";
static const char* kVersion = "0.1.0";
static const double kPollSeconds = 2.5;

// -----------------------------------------------------------------------------
// Bridge connection config (from ~/.nexora/bridge.json)
// -----------------------------------------------------------------------------
struct BridgeConfig {
    std::string host = "127.0.0.1";
    int         port = 48757;
    std::string token;
};

static BridgeConfig g_cfg;
static MCallbackId  g_timerId = 0;

static std::string homeDir() {
#ifdef _WIN32
    const char* h = std::getenv("USERPROFILE");
#else
    const char* h = std::getenv("HOME");
#endif
    return h ? std::string(h) : std::string();
}

static bool loadConfig() {
    std::string path = homeDir() + "/.nexora/bridge.json";
    FILE* fp = std::fopen(path.c_str(), "rb");
    if (!fp) return false;
    std::string data;
    char buf[4096];
    size_t n;
    while ((n = std::fread(buf, 1, sizeof(buf), fp)) > 0) data.append(buf, n);
    std::fclose(fp);
    try {
        json j = json::parse(data);
        g_cfg.host  = j.value("host", "127.0.0.1");
        g_cfg.port  = j.value("port", 48757);
        g_cfg.token = j.value("token", "");
        return true;
    } catch (...) {
        return false;
    }
}

// -----------------------------------------------------------------------------
// Minimal HTTP/1.1 client (localhost). Sends "Connection: close" and reads to
// EOF, so no chunked/keep-alive handling is needed.
// -----------------------------------------------------------------------------
static bool httpRequest(const std::string& method,
                        const std::string& path,
                        const std::string& body,     // empty for GET
                        std::string&       outBody,
                        double             timeoutSec = 3.0) {
#ifdef _WIN32
    WSADATA wsa;
    static bool wsaInit = false;
    if (!wsaInit) { if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) return false; wsaInit = true; }
#endif
    nexora_socket_t sock = ::socket(AF_INET, SOCK_STREAM, 0);
    if (sock == INVALID_SOCKET) return false;

    // Timeouts.
#ifdef _WIN32
    DWORD tv = (DWORD)(timeoutSec * 1000);
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, (const char*)&tv, sizeof(tv));
    setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, (const char*)&tv, sizeof(tv));
#else
    struct timeval tv; tv.tv_sec = (int)timeoutSec; tv.tv_usec = 0;
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
#endif

    sockaddr_in addr;
    std::memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port   = htons((unsigned short)g_cfg.port);
    inet_pton(AF_INET, g_cfg.host.c_str(), &addr.sin_addr);

    if (::connect(sock, (sockaddr*)&addr, sizeof(addr)) != 0) {
        NEXORA_CLOSESOCK(sock);
        return false;
    }

    std::string req = method + " " + path + " HTTP/1.1\r\n";
    req += "Host: " + g_cfg.host + "\r\n";
    req += "X-NEXORA-Token: " + g_cfg.token + "\r\n";
    req += "Connection: close\r\n";
    if (!body.empty()) {
        req += "Content-Type: application/json\r\n";
        req += "Content-Length: " + std::to_string(body.size()) + "\r\n";
    }
    req += "\r\n";
    if (!body.empty()) req += body;

    if (::send(sock, req.data(), (int)req.size(), 0) < 0) {
        NEXORA_CLOSESOCK(sock);
        return false;
    }

    std::string resp;
    char buf[4096];
    int r;
    while ((r = ::recv(sock, buf, sizeof(buf), 0)) > 0) resp.append(buf, r);
    NEXORA_CLOSESOCK(sock);

    // Split headers / body and check status.
    size_t hdrEnd = resp.find("\r\n\r\n");
    if (hdrEnd == std::string::npos) return false;
    std::string statusLine = resp.substr(0, resp.find("\r\n"));
    outBody = resp.substr(hdrEnd + 4);
    return statusLine.find(" 200") != std::string::npos ||
           statusLine.find(" 201") != std::string::npos;
}

static bool apiGet(const std::string& path, json& out) {
    std::string body;
    if (!httpRequest("GET", path, "", body)) return false;
    try { out = json::parse(body); return true; } catch (...) { return false; }
}

static bool apiPost(const std::string& path, const json& payload, json& out) {
    std::string body;
    if (!httpRequest("POST", path, payload.dump(), body)) return false;
    try { out = json::parse(body); return true; } catch (...) { return false; }
}

// -----------------------------------------------------------------------------
// MEL helpers (scene/shader work). Sourced once at plug-in load. Mirrors the
// verified Python shader logic: standardSurface / aiStandardSurface / VRayMtl,
// tangent-space normals via bump2d, displacement, colour-managed file nodes.
// -----------------------------------------------------------------------------
static const char* kMelHelpers = R"MEL(
// Resolve the target renderer: honor $pref if that renderer is loaded, else
// auto-detect (Arnold > V-Ray > generic). Keeps renderer choice in one place.
global proc string nexoraResolveRenderer(string $pref) {
    int $hasV = `pluginInfo -q -loaded "vrayformaya"`;
    int $hasA = `pluginInfo -q -loaded "mtoa"`;
    if ($pref == "vray" && $hasV) return "vray";
    if ($pref == "arnold" && $hasA) return "arnold";
    if ($pref == "generic_pbr") return "generic";
    if ($hasA) return "arnold";
    if ($hasV) return "vray";
    return "generic";
}

// Dedicated per-renderer slot wiring (spec §37: V-Ray logic lives here, not
// scattered). $fn = file node, $shader/$sg = target shader + shading group.
global proc nexoraConnectSlot(string $renderer, string $slot, string $fn,
                              string $shader, string $sg, int $isColor) {
    if ($slot == "normal") {
        if ($renderer == "vray") {
            // V-Ray adapter: normal map via VRayMtl.bumpMap, type 1 = tangent normal.
            catchQuiet(`connectAttr -f ($fn + ".outColor") ($shader + ".bumpMap")`);
            catchQuiet(`setAttr ($shader + ".bumpMapType") 1`);
        } else if ($renderer == "arnold") {
            // Arnold adapter: dedicated aiNormalMap (tangent-space) → normalCamera.
            string $nm = `shadingNode -asUtility aiNormalMap`;
            catchQuiet(`connectAttr -f ($fn + ".outColor") ($nm + ".input")`);
            catchQuiet(`connectAttr -f ($nm + ".outValue") ($shader + ".normalCamera")`);
        } else {
            string $b = `shadingNode -asUtility bump2d`;
            catchQuiet(`setAttr ($b + ".bumpInterp") 1`);
            catchQuiet(`connectAttr -f ($fn + ".outAlpha") ($b + ".bumpValue")`);
            catchQuiet(`connectAttr -f ($b + ".outNormal") ($shader + ".normalCamera")`);
        }
        return;
    }
    if ($slot == "bump") {
        if ($renderer == "vray") {
            catchQuiet(`connectAttr -f ($fn + ".outColor") ($shader + ".bumpMap")`);
            catchQuiet(`setAttr ($shader + ".bumpMapType") 0`);
        } else if ($renderer == "arnold") {
            // Arnold adapter: dedicated aiBump2d for height-as-bump.
            string $bb = `shadingNode -asUtility aiBump2d`;
            catchQuiet(`connectAttr -f ($fn + ".outR") ($bb + ".bumpMap")`);
            catchQuiet(`connectAttr -f ($bb + ".outValue") ($shader + ".normalCamera")`);
        } else {
            string $b = `shadingNode -asUtility bump2d`;
            catchQuiet(`connectAttr -f ($fn + ".outAlpha") ($b + ".bumpValue")`);
            catchQuiet(`connectAttr -f ($b + ".outNormal") ($shader + ".normalCamera")`);
        }
        return;
    }
    if ($slot == "height" || $slot == "displacement") {
        // Standard displacement on the shading group (honored by V-Ray/Arnold).
        string $d = `shadingNode -asShader displacementShader`;
        catchQuiet(`connectAttr -f ($fn + ".outAlpha") ($d + ".displacement")`);
        catchQuiet(`connectAttr -f ($d + ".displacement") ($sg + ".displacementShader")`);
        return;
    }
    string $attr = "";
    if ($renderer == "vray") {
        // VRayMtl. useRoughness (set on the shader) makes reflectionGlossiness
        // read the roughness map directly.
        if ($slot == "base_color") $attr = "color";
        else if ($slot == "roughness") $attr = "reflectionGlossiness";
        else if ($slot == "metallic") $attr = "metalness";
    } else if ($renderer == "arnold") {
        if ($slot == "base_color") $attr = "baseColor";
        else if ($slot == "roughness") $attr = "specularRoughness";
        else if ($slot == "metallic") $attr = "metalness";
        else if ($slot == "emission") $attr = "emissionColor";
    } else {
        if ($slot == "base_color") $attr = "baseColor";
        else if ($slot == "roughness") $attr = "specularRoughness";
        else if ($slot == "metallic") $attr = "metalness";
        else if ($slot == "emission") $attr = "emissionColor";
    }
    if ($attr != "") {
        string $out = ($isColor ? ".outColor" : ".outColorR");
        catchQuiet(`connectAttr -f ($fn + $out) ($shader + "." + $attr)`);
    }
}

global proc nexoraBuildAndAssign(string $name, string $slots[], string $paths[],
                                 int $udim[], string $rendererPref) {
    string $safe = substituteAllString($name, " ", "_");
    string $renderer = nexoraResolveRenderer($rendererPref);

    string $shader;
    if ($renderer == "arnold") {
        $shader = `shadingNode -asShader aiStandardSurface -name ($safe + "_ai")`;
    } else if ($renderer == "vray") {
        $shader = `shadingNode -asShader VRayMtl -name ($safe + "_vray")`;
        catchQuiet(`setAttr ($shader + ".useRoughness") 1`);  // treat glossiness map as roughness
    } else {
        $shader = `shadingNode -asShader standardSurface -name ($safe + "_std")`;
    }

    string $sg = `sets -renderable true -noSurfaceShader true -empty -name ($safe + "_SG")`;
    connectAttr -f ($shader + ".outColor") ($sg + ".surfaceShader");

    int $n = size($slots);
    for ($i = 0; $i < $n; $i++) {
        string $slot = $slots[$i];
        string $path = $paths[$i];
        if ($path == "") continue;
        int $isColor = ($slot == "base_color" || $slot == "emission");
        int $isUdim = ($i < size($udim)) ? $udim[$i] : 0;
        if ($isUdim) $path = substituteAllString($path, "<UDIM>", "1001");

        string $fn = `shadingNode -asTexture -isColorManaged file`;
        string $p2d = `shadingNode -asUtility place2dTexture`;
        catchQuiet(`connectAttr -f ($p2d + ".outUV") ($fn + ".uvCoord")`);
        catchQuiet(`connectAttr -f ($p2d + ".outUvFilterSize") ($fn + ".uvFilterSize")`);
        setAttr -type "string" ($fn + ".fileTextureName") $path;
        if ($isUdim) catchQuiet(`setAttr ($fn + ".uvTilingMode") 3`);  // 3 = UDIM (Mari)
        catchQuiet(`setAttr ($fn + ".ignoreColorSpaceFileRules") 1`);
        if ($isColor) catchQuiet(`setAttr -type "string" ($fn + ".colorSpace") "sRGB"`);
        else catchQuiet(`setAttr -type "string" ($fn + ".colorSpace") "Raw"`);

        nexoraConnectSlot($renderer, $slot, $fn, $shader, $sg, $isColor);
    }

    string $sel[] = `ls -sl -l`;
    int $assigned = 0;
    for ($o in $sel) {
        string $shapes[] = `listRelatives -s -f $o`;
        if (size($shapes) == 0) $shapes = {$o};
        for ($sh in $shapes) if (catch(`sets -e -forceElement $sg $sh`) == 0) $assigned++;
    }
    print("NEXORA: built '" + $name + "' [" + $renderer + "] and assigned to " + $assigned + " object(s).\n");
}

global proc nexoraApplyTexture(string $name, string $path, int $isColor) {
    string $fn = `shadingNode -asTexture -isColorManaged file`;
    string $p2d = `shadingNode -asUtility place2dTexture`;
    catchQuiet(`connectAttr -f ($p2d + ".outUV") ($fn + ".uvCoord")`);
    catchQuiet(`connectAttr -f ($p2d + ".outUvFilterSize") ($fn + ".uvFilterSize")`);
    setAttr -type "string" ($fn + ".fileTextureName") $path;
    catchQuiet(`setAttr ($fn + ".ignoreColorSpaceFileRules") 1`);
    if ($isColor) catchQuiet(`setAttr -type "string" ($fn + ".colorSpace") "sRGB"`);
    else catchQuiet(`setAttr -type "string" ($fn + ".colorSpace") "Raw"`);
    select -r $fn;
    print("NEXORA: created file node for '" + $name + "'.\n");
}

global proc string[] nexoraCaptureMaps() {
    string $result[];
    string $sel[] = `ls -sl -l`;
    if (size($sel) == 0) return $result;
    string $shader = "";
    string $shapes[] = `listRelatives -s -f $sel[0]`;
    if (size($shapes) == 0) $shapes = {$sel[0]};
    for ($sh in $shapes) {
        string $sgs[] = `listConnections -type shadingEngine $sh`;
        for ($sg in $sgs) {
            string $srf[] = `listConnections ($sg + ".surfaceShader")`;
            if (size($srf) > 0) { $shader = $srf[0]; break; }
        }
        if ($shader != "") break;
    }
    if ($shader == "" && `objExists $sel[0]`) {
        string $ot = `objectType $sel[0]`;
        if (`gmatch $ot "*Surface"`) $shader = $sel[0];
    }
    if ($shader == "") return $result;

    string $attrs[] = {"baseColor","color","diffuseColor","specularRoughness","reflectionGlossiness",
                       "metalness","normalCamera","bumpMap","emissionColor"};
    string $slots[] = {"base_color","base_color","base_color","roughness","roughness",
                       "metallic","normal","normal","emission"};
    string $seen = "|";
    for ($i = 0; $i < size($attrs); $i++) {
        string $plug = $shader + "." + $attrs[$i];
        if (!`objExists $plug`) continue;
        if (`gmatch $seen ("*|" + $slots[$i] + "|*")`) continue;
        string $src[] = `listConnections -s 1 -d 0 $plug`;
        for ($s in $src) {
            string $hist[] = `listHistory $s`;
            string $files[] = `ls -type file $hist`;
            if (size($files) > 0) {
                string $p = `getAttr ($files[0] + ".fileTextureName")`;
                if ($p != "") { $result[size($result)] = ($slots[$i] + "|" + $p); $seen += ($slots[$i] + "|"); break; }
            }
        }
    }
    return $result;
}
)MEL";

// -----------------------------------------------------------------------------
// Small helpers
// -----------------------------------------------------------------------------
static std::string melEscape(const std::string& in) {
    std::string out;
    for (char c : in) {
        if (c == '\\') out += '/';       // forward slashes are safe in MEL strings
        else if (c == '"') { out += '\\'; out += '"'; }
        else out += c;
    }
    return out;
}

static std::string melStringArray(const std::vector<std::string>& v) {
    std::string s = "{";
    for (size_t i = 0; i < v.size(); ++i) {
        if (i) s += ",";
        s += "\"" + melEscape(v[i]) + "\"";
    }
    s += "}";
    return s;
}

// MEL int array literal, e.g. {0,1,0}. Empty vector -> a valid empty MEL array.
static std::string melIntArray(const std::vector<int>& v) {
    if (v.empty()) return "{}";
    std::string s = "{";
    for (size_t i = 0; i < v.size(); ++i) {
        if (i) s += ",";
        s += std::to_string(v[i]);
    }
    s += "}";
    return s;
}

// Fetch a texture's file path + UDIM flag from the API (materials only carry
// texture ids). Returns false if the texture can't be resolved.
static bool textureInfo(const std::string& textureId, std::string& path, bool& isUdim) {
    json t;
    if (!apiGet("/api/textures/" + textureId, t)) return false;
    if (!t.contains("file_path") || !t["file_path"].is_string()) return false;
    path = t["file_path"].get<std::string>();
    // Prefer the DB flag; fall back to the presence of a <UDIM> token in the path.
    isUdim = (t.contains("is_udim") && t["is_udim"].is_boolean() && t["is_udim"].get<bool>())
             || path.find("<UDIM>") != std::string::npos;
    return !path.empty();
}

// The renderer preference NEXORA is set to, from /api/status ("generic_pbr",
// "arnold", "vray"). The MEL resolver honors it if that renderer is loaded, else
// auto-detects — so a stale/unreachable status just means "auto".
static std::string currentRendererPref() {
    json s;
    if (!apiGet("/api/status", s)) return "generic_pbr";
    if (s.contains("default_renderer") && s["default_renderer"].is_string())
        return s["default_renderer"].get<std::string>();
    return "generic_pbr";
}

// Build a material's shader network from its JSON and assign to the selection.
static void applyMaterialJson(const json& material) {
    std::string name = material.value("name", "NEXORA_Material");
    std::vector<std::string> slots, paths;
    std::vector<int> udims;
    if (material.contains("maps") && material["maps"].is_array()) {
        for (const auto& m : material["maps"]) {
            std::string slot = m.value("slot", "");
            std::string tid  = m.value("texture_id", "");
            if (slot.empty() || tid.empty()) continue;
            std::string p; bool isUdim = false;
            if (!textureInfo(tid, p, isUdim)) continue;
            slots.push_back(slot);
            paths.push_back(p);
            udims.push_back(isUdim ? 1 : 0);
        }
    }
    std::string pref = currentRendererPref();
    std::string mel = "nexoraBuildAndAssign(\"" + melEscape(name) + "\", " +
                      melStringArray(slots) + ", " + melStringArray(paths) + ", " +
                      melIntArray(udims) + ", \"" + melEscape(pref) + "\");";
    MGlobal::executeCommand(MString(mel.c_str()), false, false);
}

static void applyTextureJson(const json& texture) {
    std::string name = texture.value("name", "NEXORA_tex");
    std::string path = texture.value("file_path", "");
    if (path.empty()) return;
    std::string mt = texture.value("map_type", "");
    int isColor = (mt == "base_color" || mt == "emission") ? 1 : 0;
    std::string mel = "nexoraApplyTexture(\"" + melEscape(name) + "\", \"" + melEscape(path) +
                      "\", " + std::to_string(isColor) + ");";
    MGlobal::executeCommand(MString(mel.c_str()), false, false);
}

// -----------------------------------------------------------------------------
// Poller: heartbeat + drain the "Send to Maya" queue
// -----------------------------------------------------------------------------
static void doSync() {
    // Heartbeat (best-effort).
    json ignore, hb;
    hb["version"] = "maya";
    apiPost("/api/heartbeat", hb, ignore);

    // Pull queued sends.
    json items;
    if (!apiGet("/api/pull", items) || !items.is_array()) return;
    for (const auto& it : items) {
        std::string kind = it.value("kind", "");
        if (kind == "material" && it.contains("material") && it["material"].is_object())
            applyMaterialJson(it["material"]);
        else if (kind == "texture" && it.contains("texture") && it["texture"].is_object())
            applyTextureJson(it["texture"]);
    }
}

static void timerCallback(float /*elapsed*/, float /*last*/, void* /*clientData*/) {
    loadConfig();
    doSync();
}

// -----------------------------------------------------------------------------
// Commands
// -----------------------------------------------------------------------------
class NexoraSyncCmd : public MPxCommand {
public:
    static void* creator() { return new NexoraSyncCmd(); }
    MStatus doIt(const MArgList&) override { loadConfig(); doSync(); return MS::kSuccess; }
};

class NexoraApplyCmd : public MPxCommand {
public:
    static void* creator() { return new NexoraApplyCmd(); }
    static MSyntax newSyntax() {
        MSyntax s;
        s.addFlag("-id", "-identifier", MSyntax::kString);
        s.addFlag("-k", "-kind", MSyntax::kString);
        return s;
    }
    MStatus doIt(const MArgList& args) override {
        MStatus st;
        MSyntax s = newSyntax();
        MArgDatabase db(s, args, &st);
        if (!st) return st;
        MString id, kind("material");
        if (db.isFlagSet("-id")) db.getFlagArgument("-id", 0, id);
        if (db.isFlagSet("-k"))  db.getFlagArgument("-k", 0, kind);
        if (id.length() == 0) { displayError("nexoraApply: -id is required"); return MS::kFailure; }

        loadConfig();
        json obj;
        if (kind == "texture") {
            if (apiGet(std::string("/api/textures/") + id.asChar(), obj)) applyTextureJson(obj);
            else displayWarning("NEXORA: could not fetch texture");
        } else {
            if (apiGet(std::string("/api/materials/") + id.asChar(), obj)) applyMaterialJson(obj);
            else displayWarning("NEXORA: could not fetch material");
        }
        return MS::kSuccess;
    }
};

class NexoraCaptureCmd : public MPxCommand {
public:
    static void* creator() { return new NexoraCaptureCmd(); }
    static MSyntax newSyntax() {
        MSyntax s;
        s.addFlag("-n", "-name", MSyntax::kString);
        return s;
    }
    MStatus doIt(const MArgList& args) override {
        MString name("Captured Material");
        MSyntax s = newSyntax();
        MArgDatabase db(s, args);
        if (db.isFlagSet("-n")) db.getFlagArgument("-n", 0, name);

        MStringArray rows;
        MGlobal::executeCommand("nexoraCaptureMaps()", rows);
        if (rows.length() == 0) { displayWarning("NEXORA: no file textures on the selection."); return MS::kSuccess; }

        json maps = json::array();
        for (unsigned i = 0; i < rows.length(); ++i) {
            std::string row = rows[i].asChar();
            size_t bar = row.find('|');
            if (bar == std::string::npos) continue;
            maps.push_back({{"slot", row.substr(0, bar)}, {"path", row.substr(bar + 1)}});
        }
        loadConfig();
        json payload; payload["name"] = name.asChar(); payload["maps"] = maps;
        json out;
        if (apiPost("/api/material/capture", payload, out) && out.value("ok", false)) {
            std::string msg = std::string("NEXORA: captured '") + name.asChar() + "'.";
            displayInfo(MString(msg.c_str()));
        } else {
            displayError("NEXORA: capture failed.");
        }
        return MS::kSuccess;
    }
};

// Build a MEL browser window populated from the API. Double-click (or the Apply
// button) calls the compiled `nexoraApply` command with the selected id.
class NexoraLibraryCmd : public MPxCommand {
public:
    static void* creator() { return new NexoraLibraryCmd(); }
    MStatus doIt(const MArgList&) override {
        loadConfig();
        json mats, texs;
        bool ok = apiGet("/api/materials", mats);
        apiGet("/api/textures", texs);
        if (!ok) { displayWarning("NEXORA desktop not reachable — is it running?"); }

        std::vector<std::string> matIds, matNames, texIds, texNames;
        if (mats.is_array()) for (const auto& m : mats) {
            matIds.push_back(m.value("id", ""));
            matNames.push_back(m.value("name", "?") + "   ·   " + m.value("category", ""));
        }
        if (texs.is_array()) for (const auto& t : texs) {
            texIds.push_back(t.value("id", ""));
            std::string mt = t.value("map_type", ""); if (mt.empty()) mt = "texture";
            texNames.push_back(t.value("name", "?") + "   ·   " + mt);
        }

        std::string mel;
        mel += "if (`window -exists nexoraWin`) deleteUI nexoraWin;\n";
        mel += "window -title \"NEXORA Library\" -widthHeight 380 520 nexoraWin;\n";
        mel += "string $tabs = `tabLayout -innerMarginWidth 4 -innerMarginHeight 4`;\n";
        mel += "string $c1 = `columnLayout -adj 1`;\n";
        mel += "  textScrollList -w 360 -h 440 -dcc \"nexoraApplyMatSel\" nexoraMatList;\n";
        mel += "  button -label \"Apply Material to Selection\" -c \"nexoraApplyMatSel\";\n";
        mel += "  setParent ..;\n";
        mel += "string $c2 = `columnLayout -adj 1`;\n";
        mel += "  textScrollList -w 360 -h 440 -dcc \"nexoraImportTexSel\" nexoraTexList;\n";
        mel += "  button -label \"Import Texture\" -c \"nexoraImportTexSel\";\n";
        mel += "  setParent ..;\n";
        mel += "setParent ..;\n";
        mel += "tabLayout -e -tabLabel $c1 \"Materials\" -tabLabel $c2 \"Textures\" $tabs;\n";

        // id arrays + selection procs
        mel += "global string $gNexoraMatIds[] = " + melStringArray(matIds) + ";\n";
        mel += "global string $gNexoraTexIds[] = " + melStringArray(texIds) + ";\n";
        for (const auto& n : matNames) mel += "textScrollList -e -append \"" + melEscape(n) + "\" nexoraMatList;\n";
        for (const auto& n : texNames) mel += "textScrollList -e -append \"" + melEscape(n) + "\" nexoraTexList;\n";
        mel += "showWindow nexoraWin;\n";
        mel += "global proc nexoraApplyMatSel() { global string $gNexoraMatIds[]; int $i[] = `textScrollList -q -sii nexoraMatList`; if (size($i) > 0) nexoraApply -id $gNexoraMatIds[$i[0]-1] -k \"material\"; }\n";
        mel += "global proc nexoraImportTexSel() { global string $gNexoraTexIds[]; int $i[] = `textScrollList -q -sii nexoraTexList`; if (size($i) > 0) nexoraApply -id $gNexoraTexIds[$i[0]-1] -k \"texture\"; }\n";

        MGlobal::executeCommand(MString(mel.c_str()), false, false);
        return MS::kSuccess;
    }
};

// -----------------------------------------------------------------------------
// Menu + shelf (built via MEL)
// -----------------------------------------------------------------------------
static void buildUi() {
    const char* mel =
        "if (`menu -exists nexoraMenu`) deleteUI -menu nexoraMenu;\n"
        "global string $gMainWindow;\n"
        "menu -parent $gMainWindow -label \"NEXORA\" -tearOff true nexoraMenu;\n"
        "menuItem -parent nexoraMenu -label \"Open NEXORA Library\" -command \"nexoraLibrary\";\n"
        "menuItem -parent nexoraMenu -divider true;\n"
        "menuItem -parent nexoraMenu -label \"Capture Material from Selection\" -command \"nexoraCapture\";\n"
        "menuItem -parent nexoraMenu -label \"Sync Now\" -command \"nexoraSync\";\n"
        "global string $gShelfTopLevel;\n"
        "if (`shelfLayout -exists NEXORA`) deleteUI NEXORA;\n"
        "shelfLayout -parent $gShelfTopLevel NEXORA;\n"
        "shelfButton -parent NEXORA -label \"Library\" -imageOverlayLabel \"NX\" "
            "-image \"menuIconWindow.png\" -sourceType \"mel\" -command \"nexoraLibrary\";\n"
        "shelfButton -parent NEXORA -label \"Capture\" -imageOverlayLabel \"CAP\" "
            "-image \"out_shadingEngine.png\" -sourceType \"mel\" -command \"nexoraCapture\";\n";
    MGlobal::executeCommand(mel, false, false);
}

static void removeUi() {
    MGlobal::executeCommand(
        "if (`menu -exists nexoraMenu`) deleteUI -menu nexoraMenu;\n"
        "if (`shelfLayout -exists NEXORA`) deleteUI NEXORA;\n"
        "if (`window -exists nexoraWin`) deleteUI nexoraWin;\n",
        false, false);
}

// -----------------------------------------------------------------------------
// Plug-in entry points
// -----------------------------------------------------------------------------
NEXORA_EXPORT MStatus initializePlugin(MObject obj) {
    MFnPlugin plugin(obj, kVendor, kVersion, "Any");
    MStatus st;

    st = plugin.registerCommand("nexoraLibrary", NexoraLibraryCmd::creator); if (!st) return st;
    st = plugin.registerCommand("nexoraApply",   NexoraApplyCmd::creator, NexoraApplyCmd::newSyntax); if (!st) return st;
    st = plugin.registerCommand("nexoraCapture", NexoraCaptureCmd::creator, NexoraCaptureCmd::newSyntax); if (!st) return st;
    st = plugin.registerCommand("nexoraSync",    NexoraSyncCmd::creator); if (!st) return st;

    loadConfig();
    MGlobal::executeCommand(kMelHelpers, false, false);  // source MEL helpers
    buildUi();

    g_timerId = MTimerMessage::addTimerCallback((float)kPollSeconds, timerCallback, nullptr, &st);

    std::string loaded = std::string("NEXORA Bridge ") + kVersion + " loaded.";
    MGlobal::displayInfo(MString(loaded.c_str()));
    return MS::kSuccess;
}

NEXORA_EXPORT MStatus uninitializePlugin(MObject obj) {
    MFnPlugin plugin(obj);

    if (g_timerId != 0) { MMessage::removeCallback(g_timerId); g_timerId = 0; }
    removeUi();

    plugin.deregisterCommand("nexoraLibrary");
    plugin.deregisterCommand("nexoraApply");
    plugin.deregisterCommand("nexoraCapture");
    plugin.deregisterCommand("nexoraSync");

    MGlobal::displayInfo("NEXORA Bridge unloaded.");
    return MS::kSuccess;
}
