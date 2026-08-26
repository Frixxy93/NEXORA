"""
NEXORA Bridge — Maya plug-in (spec §32-§39).

A real Maya scripted plug-in (Python API 2.0) that connects Maya to the NEXORA
desktop app over its localhost Bridge API. It:

  * registers a Maya command ``nexoraLibrary`` (opens the library browser),
  * adds a **NEXORA** menu + shelf,
  * runs a poller that heartbeats NEXORA ("Maya connected") and drains the
    desktop's "Send to Maya" queue, applying materials/textures automatically,
  * can capture the selected Maya shader back into NEXORA.

It never touches NEXORA's database directly — everything goes through the HTTP
Bridge API. Host/port/token are read from ``~/.nexora/bridge.json`` (written by
the desktop app on startup).

Verified against Autodesk's Maya API docs for: scripted-plugin entry points
(``maya_useNewAPI`` / ``initializePlugin`` / ``uninitializePlugin`` / MFnPlugin,
API 2.0), top-level menus (``$gMainWindow``), custom shelves
(``$gShelfTopLevel``), and shading-network construction (file → standardSurface /
aiStandardSurface / VRayMtl, tangent-space normals via ``bump2d``).

Install: copy this file into a Maya plug-in path (e.g.
``Documents/maya/<version>/plug-ins``) and enable it in the Plug-in Manager.
See README.md.

Author: FRIXXY · MIT
"""

import json
import os
import urllib.request

import maya.api.OpenMaya as om
import maya.cmds as cmds
import maya.mel as mel
import maya.OpenMayaUI as omui
import maya.utils

PLUGIN_NAME = "NEXORA Bridge"
PLUGIN_VERSION = "0.1.0"
COMMAND_NAME = "nexoraLibrary"
MENU_NAME = "nexoraMenu"
SHELF_NAME = "NEXORA"
POLL_MS = 2500


# Tell Maya to use the Python API 2.0 for this plug-in.
def maya_useNewAPI():
    pass


# --- PySide (2 or 6, i.e. Maya 2024- vs 2025+) ------------------------------
try:
    from PySide6 import QtWidgets, QtCore
    from shiboken6 import wrapInstance
except ImportError:
    from PySide2 import QtWidgets, QtCore
    from shiboken2 import wrapInstance


def maya_main_window():
    ptr = omui.MQtUtil.mainWindow()
    return wrapInstance(int(ptr), QtWidgets.QWidget) if ptr else None


# ---------------------------------------------------------------------------
# Bridge client
# ---------------------------------------------------------------------------
class NexoraClient(object):
    """Thin HTTP client for the NEXORA Bridge API."""

    def __init__(self):
        self.host = "127.0.0.1"
        self.port = 48757
        self.token = ""
        self.reload_config()

    def reload_config(self):
        path = os.path.join(os.path.expanduser("~"), ".nexora", "bridge.json")
        try:
            with open(path, "r") as fh:
                cfg = json.load(fh)
            self.host = cfg.get("host", "127.0.0.1")
            self.port = int(cfg.get("port", 48757))
            self.token = cfg.get("token", "")
            return True
        except Exception:
            return False

    def _request(self, method, endpoint, payload=None, timeout=3.0):
        url = "http://{0}:{1}{2}".format(self.host, self.port, endpoint)
        data = None
        headers = {"X-NEXORA-Token": self.token}
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8")
        return json.loads(body) if body else {}

    def get(self, endpoint, timeout=3.0):
        return self._request("GET", endpoint, timeout=timeout)

    def post(self, endpoint, payload, timeout=6.0):
        return self._request("POST", endpoint, payload=payload, timeout=timeout)

    # convenience -----------------------------------------------------------
    def status(self):
        return self.get("/api/status", timeout=1.5)

    def heartbeat(self):
        try:
            return self.post("/api/heartbeat", {"version": cmds.about(version=True)}, timeout=1.5)
        except Exception:
            return None

    def materials(self):
        return self.get("/api/materials")

    def textures(self):
        return self.get("/api/textures")

    def texture(self, texture_id):
        return self.get("/api/textures/" + texture_id)

    def pull(self):
        try:
            return self.get("/api/pull", timeout=1.5)
        except Exception:
            return []

    def capture(self, name, maps):
        return self.post("/api/material/capture", {"name": name, "maps": maps})


CLIENT = NexoraClient()


# ---------------------------------------------------------------------------
# Shading network construction (spec §34-§36)
# ---------------------------------------------------------------------------
def _renderer_pref():
    """The renderer NEXORA is configured for (/api/status default_renderer):
    "arnold", "vray", or "generic_pbr". Falls back to "generic_pbr" if the
    desktop is unreachable — which the resolver treats as auto-detect."""
    try:
        return (CLIENT.status() or {}).get("default_renderer") or "generic_pbr"
    except Exception:
        return "generic_pbr"


def _renderer(pref=None):
    """Resolve the target renderer. Honor ``pref`` if that renderer is actually
    loaded; otherwise auto-detect (Arnold > V-Ray > generic). Mirrors the C++
    plug-in's ``nexoraResolveRenderer`` so both variants pick the same target."""
    def loaded(name):
        try:
            return bool(cmds.pluginInfo(name, q=True, loaded=True))
        except Exception:
            return False

    has_arnold = loaded("mtoa")
    has_vray = loaded("vrayformaya")
    if pref == "arnold" and has_arnold:
        return "arnold"
    if pref == "vray" and has_vray:
        return "vray"
    if pref == "generic_pbr":
        return "generic"
    if has_arnold:
        return "arnold"
    if has_vray:
        return "vray"
    return "generic"


def _make_file_node(path, name, raw=True, is_udim=False):
    """Create a file + place2dTexture pair; return the file node.

    When ``is_udim`` is set, the ``<UDIM>`` token is swapped to the first tile
    (1001) for the file path and the node is switched to UDIM (Mari) tiling."""
    if is_udim:
        path = path.replace("<UDIM>", "1001")
    file_node = cmds.shadingNode("file", asTexture=True, isColorManaged=True, name=name)
    p2d = cmds.shadingNode("place2dTexture", asUtility=True, name=name + "_p2d")
    links = [
        "coverage", "translateFrame", "rotateFrame", "mirrorU", "mirrorV", "stagger",
        "wrapU", "wrapV", "repeatUV", "offset", "rotateUV", "noiseUV", "vertexUvOne",
        "vertexUvTwo", "vertexUvThree", "vertexCameraOne",
    ]
    for attr in links:
        try:
            cmds.connectAttr(p2d + "." + attr, file_node + "." + attr, f=True)
        except Exception:
            pass
    for src, dst in (("outUV", "uvCoord"), ("outUvFilterSize", "uvFilterSize")):
        try:
            cmds.connectAttr(p2d + "." + src, file_node + "." + dst, f=True)
        except Exception:
            pass
    cmds.setAttr(file_node + ".fileTextureName", path, type="string")
    if is_udim:
        try:
            cmds.setAttr(file_node + ".uvTilingMode", 3)  # 3 = UDIM (Mari)
        except Exception:
            pass
    # Force our color space rather than letting file rules override it.
    try:
        cmds.setAttr(file_node + ".ignoreColorSpaceFileRules", 1)
        cmds.setAttr(file_node + ".colorSpace", "Raw" if raw else "sRGB", type="string")
    except Exception:
        pass
    return file_node


def _connect_normal(file_node, shader, renderer):
    """Wire a tangent-space normal map into the shader (§36)."""
    if renderer == "vray":
        # VRayMtl uses a bumpMap slot with bumpMapType = 1 (normal map).
        try:
            cmds.connectAttr(file_node + ".outColor", shader + ".bumpMap", f=True)
            cmds.setAttr(shader + ".bumpMapType", 1)
        except Exception:
            pass
        return
    if renderer == "arnold":
        # Arnold adapter: dedicated aiNormalMap (tangent-space) → normalCamera.
        nm = cmds.shadingNode("aiNormalMap", asUtility=True)
        try:
            cmds.connectAttr(file_node + ".outColor", nm + ".input", f=True)
            cmds.connectAttr(nm + ".outValue", shader + ".normalCamera", f=True)
        except Exception:
            pass
        return
    # Generic: bump2d in tangent-space-normal mode → normalCamera.
    bump = cmds.shadingNode("bump2d", asUtility=True)
    try:
        cmds.setAttr(bump + ".bumpInterp", 1)  # 1 = Tangent Space Normals
        cmds.connectAttr(file_node + ".outAlpha", bump + ".bumpValue", f=True)
        cmds.connectAttr(bump + ".outNormal", shader + ".normalCamera", f=True)
    except Exception:
        pass


def _connect_displacement(file_node, shading_engine):
    disp = cmds.shadingNode("displacementShader", asShader=True)
    try:
        cmds.connectAttr(file_node + ".outAlpha", disp + ".displacement", f=True)
        cmds.connectAttr(disp + ".displacement", shading_engine + ".displacementShader", f=True)
    except Exception:
        pass


# Per-renderer scalar/colour slot → shader attribute. Kept in one table so the
# V-Ray adapter (and Arnold, generic) stays in a single place rather than being
# scattered through the builder (spec §37).
_SLOT_ATTRS = {
    "vray":    {"base_color": "color", "roughness": "reflectionGlossiness",
                "metallic": "metalness"},
    "arnold":  {"base_color": "baseColor", "roughness": "specularRoughness",
                "metallic": "metalness", "emission": "emissionColor"},
    "generic": {"base_color": "baseColor", "roughness": "specularRoughness",
                "metallic": "metalness", "emission": "emissionColor"},
}


def _connect_ao_basecolor(ao_node, bc_node, shader, renderer):
    """Wire an ambient-occlusion map by MULTIPLYING it into the base colour.

    No surface shader (standardSurface / aiStandardSurface / VRayMtl) has a
    dedicated AO input, so the physically-correct place for a baked AO map is
    modulating the diffuse albedo: ``base_color * ao``. A ``multiplyDivide`` node
    does this in one hop and works across all three renderers. If there is no
    base-colour map, the AO map alone drives the colour (multiplied against
    white), so the map is never silently dropped.
    """
    attr = _SLOT_ATTRS.get(renderer, {}).get("base_color")
    if not attr:
        return
    try:
        mult = cmds.shadingNode("multiplyDivide", asUtility=True)
        cmds.setAttr(mult + ".operation", 1)  # 1 = multiply
        # AO (grayscale) modulates every channel of the base colour.
        cmds.connectAttr(ao_node + ".outColor", mult + ".input2", f=True)
        if bc_node is not None:
            cmds.connectAttr(bc_node + ".outColor", mult + ".input1", f=True)
        else:
            # No albedo map: multiply against white so AO alone tints the shader.
            cmds.setAttr(mult + ".input1", 1.0, 1.0, 1.0, type="double3")
        cmds.connectAttr(mult + ".output", shader + "." + attr, f=True)
    except Exception:
        pass


def _connect_slot(file_node, slot, shader, sg, renderer, is_color):
    """Dedicated per-renderer wiring for one map. Mirrors the C++
    ``nexoraConnectSlot`` — normal/bump/displacement and the scalar/colour slots,
    with the V-Ray specifics (``bumpMap``/``bumpMapType``) confined here."""
    if slot == "normal":
        _connect_normal(file_node, shader, renderer)
        return
    if slot == "bump":
        if renderer == "vray":
            try:
                cmds.connectAttr(file_node + ".outColor", shader + ".bumpMap", f=True)
                cmds.setAttr(shader + ".bumpMapType", 0)  # 0 = bump (height)
            except Exception:
                pass
        elif renderer == "arnold":
            # Arnold adapter: dedicated aiBump2d for height-as-bump.
            bb = cmds.shadingNode("aiBump2d", asUtility=True)
            try:
                cmds.connectAttr(file_node + ".outColorR", bb + ".bumpMap", f=True)
                cmds.connectAttr(bb + ".outValue", shader + ".normalCamera", f=True)
            except Exception:
                pass
        else:
            bump = cmds.shadingNode("bump2d", asUtility=True)
            try:
                cmds.connectAttr(file_node + ".outAlpha", bump + ".bumpValue", f=True)
                cmds.connectAttr(bump + ".outNormal", shader + ".normalCamera", f=True)
            except Exception:
                pass
        return
    if slot in ("height", "displacement"):
        _connect_displacement(file_node, sg)
        return
    attr = _SLOT_ATTRS.get(renderer, {}).get(slot)
    if attr:
        out = ".outColor" if is_color else ".outColorR"
        try:
            cmds.connectAttr(file_node + out, shader + "." + attr, f=True)
        except Exception:
            pass


def build_shader(material):
    """Build a shader network from a NEXORA material dict; return (shader, SG)."""
    name = (material.get("name") or "NEXORA_Material").replace(" ", "_")
    renderer = _renderer(_renderer_pref())

    if renderer == "arnold":
        shader = cmds.shadingNode("aiStandardSurface", asShader=True, name=name + "_ai")
    elif renderer == "vray":
        shader = cmds.shadingNode("VRayMtl", asShader=True, name=name + "_vray")
        try:
            # Treat the glossiness slot as a roughness map (V-Ray adapter, §37).
            cmds.setAttr(shader + ".useRoughness", 1)
        except Exception:
            pass
    else:
        shader = cmds.shadingNode("standardSurface", asShader=True, name=name + "_std")

    sg = cmds.sets(renderable=True, noSurfaceShader=True, empty=True, name=name + "_SG")
    cmds.connectAttr(shader + ".outColor", sg + ".surfaceShader", f=True)

    maps = {m["slot"]: m for m in material.get("maps", [])}

    def tex_for(texture_id):
        """Return (path, is_udim) for a texture id, or (None, False)."""
        try:
            t = CLIENT.texture(texture_id) or {}
        except Exception:
            return None, False
        path = t.get("file_path")
        is_udim = bool(t.get("is_udim")) or (path and "<UDIM>" in path)
        return path, bool(is_udim)

    # AO has no dedicated shader slot; it's multiplied into base_color below. So
    # build its file node here but defer wiring, and — when AO is present — defer
    # base_color too so the multiply drives the colour instead of a direct link.
    has_ao = "ao" in maps
    ao_fn = None
    bc_fn = None

    for slot, node in maps.items():
        path, is_udim = tex_for(node.get("texture_id", ""))
        if not path:
            continue
        is_color = slot in ("base_color", "emission")
        try:
            fn = _make_file_node(path, name + "_" + slot, raw=not is_color, is_udim=is_udim)
            if slot == "ao":
                ao_fn = fn
                continue
            if slot == "base_color" and has_ao:
                bc_fn = fn  # wired through the AO multiply, not directly
                continue
            _connect_slot(fn, slot, shader, sg, renderer, is_color)
        except Exception as exc:
            om.MGlobal.displayWarning("NEXORA: could not connect %s (%s)" % (slot, exc))

    # base_color * ao → shader colour (only when an AO map is present).
    if ao_fn is not None:
        _connect_ao_basecolor(ao_fn, bc_fn, shader, renderer)

    return shader, sg


def apply_material(material):
    """Build the material and assign it to the current selection (§34)."""
    shader, sg = build_shader(material)
    sel = cmds.ls(selection=True, long=True) or []
    assigned = 0
    for node in sel:
        shapes = cmds.listRelatives(node, shapes=True, fullPath=True) or [node]
        for shape in shapes:
            try:
                cmds.sets(shape, edit=True, forceElement=sg)
                assigned += 1
            except Exception:
                pass
    tail = " → assigned to %d object(s)." % assigned if assigned else " (select an object to assign it)."
    om.MGlobal.displayInfo("NEXORA: built '%s'%s" % (material.get("name", "material"), tail))
    return shader


def apply_texture(texture):
    """Create a file node for a texture (spec §35)."""
    path = texture.get("file_path")
    if not path:
        return None
    raw = texture.get("map_type") not in ("base_color", "emission")
    fn = _make_file_node(path, (texture.get("name") or "NEXORA_tex").replace(" ", "_"), raw=raw)
    cmds.select(fn, replace=True)
    om.MGlobal.displayInfo("NEXORA: created file node for '%s'." % texture.get("name", "texture"))
    return fn


# ---------------------------------------------------------------------------
# Capture (Maya → NEXORA) — spec §39
# ---------------------------------------------------------------------------
_ATTR_TO_SLOT = {
    "baseColor": "base_color", "color": "base_color", "diffuseColor": "base_color",
    "specularRoughness": "roughness", "reflectionGlossiness": "roughness",
    "metalness": "metallic", "normalCamera": "normal", "bumpMap": "normal",
    "emissionColor": "emission",
}


def capture_selected_material(name=None):
    """Read the selected object's shader and send its maps to NEXORA (§39)."""
    sel = cmds.ls(selection=True, long=True)
    if not sel:
        om.MGlobal.displayWarning("NEXORA: select an object (or shader) to capture.")
        return

    shader = None
    node = sel[0]
    shapes = cmds.listRelatives(node, shapes=True, fullPath=True) or [node]
    for shape in shapes:
        for sg in cmds.listConnections(shape, type="shadingEngine") or []:
            srf = cmds.listConnections(sg + ".surfaceShader") or []
            if srf:
                shader = srf[0]
                break
        if shader:
            break
    if shader is None and cmds.objectType(node).endswith("Surface"):
        shader = node
    if shader is None:
        om.MGlobal.displayWarning("NEXORA: no surface shader found on the selection.")
        return

    maps = []
    seen = set()
    for attr, slot in _ATTR_TO_SLOT.items():
        plug = shader + "." + attr
        if slot in seen or not cmds.objExists(plug):
            continue
        for src in cmds.listConnections(plug, source=True, destination=False) or []:
            files = cmds.ls(cmds.listHistory(src) or [], type="file") or []
            if files:
                path = cmds.getAttr(files[0] + ".fileTextureName")
                if path:
                    maps.append({"slot": slot, "path": path})
                    seen.add(slot)
                    break

    if not maps:
        om.MGlobal.displayWarning("NEXORA: no file textures connected to '%s'." % shader)
        return
    try:
        res = CLIENT.capture(name or shader, maps)
        om.MGlobal.displayInfo("NEXORA: captured '%s' (%d maps)." % (name or shader, len(maps)))
        return res
    except Exception as exc:
        om.MGlobal.displayError("NEXORA: capture failed (%s)." % exc)


def scan_scene():
    files = cmds.ls(type="file") or []
    missing = [(f, cmds.getAttr(f + ".fileTextureName"))
               for f in files
               if cmds.getAttr(f + ".fileTextureName") and not os.path.exists(cmds.getAttr(f + ".fileTextureName"))]
    om.MGlobal.displayInfo("NEXORA: %d file textures, %d missing." % (len(files), len(missing)))
    for f, path in missing:
        om.MGlobal.displayWarning("  missing: %s -> %s" % (f, path))
    return files, missing


# ---------------------------------------------------------------------------
# Poller: heartbeat + drain the send queue
# ---------------------------------------------------------------------------
_TIMER = None


def _poll():
    CLIENT.heartbeat()
    for item in CLIENT.pull() or []:
        try:
            if item.get("kind") == "material" and item.get("material"):
                apply_material(item["material"])
            elif item.get("kind") == "texture" and item.get("texture"):
                apply_texture(item["texture"])
        except Exception as exc:
            om.MGlobal.displayError("NEXORA: apply failed (%s)." % exc)


def _start_poller():
    global _TIMER
    if _TIMER is not None:
        return
    _TIMER = QtCore.QTimer(maya_main_window())
    _TIMER.setInterval(POLL_MS)
    _TIMER.timeout.connect(_poll)
    _TIMER.start()


def _stop_poller():
    global _TIMER
    if _TIMER is not None:
        _TIMER.stop()
        _TIMER.deleteLater()
        _TIMER = None


# ---------------------------------------------------------------------------
# Library browser (spec §33)
# ---------------------------------------------------------------------------
_WINDOW = None


class NexoraBrowser(QtWidgets.QDialog):
    def __init__(self, parent=None):
        super(NexoraBrowser, self).__init__(parent or maya_main_window())
        self.setWindowTitle("NEXORA Library")
        self.setMinimumSize(380, 500)
        self._build_ui()
        self.refresh()

    def _build_ui(self):
        layout = QtWidgets.QVBoxLayout(self)
        self.status = QtWidgets.QLabel("Connecting…")
        self.status.setWordWrap(True)
        layout.addWidget(self.status)

        self.search = QtWidgets.QLineEdit()
        self.search.setPlaceholderText("Filter…")
        self.search.textChanged.connect(self._filter)
        layout.addWidget(self.search)

        self.tabs = QtWidgets.QTabWidget()
        self.mat_list = QtWidgets.QListWidget()
        self.tex_list = QtWidgets.QListWidget()
        self.mat_list.itemDoubleClicked.connect(lambda it: apply_material(it.data(QtCore.Qt.UserRole)))
        self.tex_list.itemDoubleClicked.connect(lambda it: apply_texture(it.data(QtCore.Qt.UserRole)))
        self.tabs.addTab(self.mat_list, "Materials")
        self.tabs.addTab(self.tex_list, "Textures")
        layout.addWidget(self.tabs)

        row = QtWidgets.QHBoxLayout()
        for label, cb in (
            ("Apply / Import", self._apply_current),
            ("Capture Selected", lambda: capture_selected_material()),
            ("Refresh", self.refresh),
        ):
            btn = QtWidgets.QPushButton(label)
            btn.clicked.connect(cb)
            row.addWidget(btn)
        layout.addLayout(row)

    def refresh(self):
        CLIENT.reload_config()
        try:
            st = CLIENT.status()
            self.status.setText("Connected · NEXORA %s · %d materials, %d textures"
                                % (st.get("version", "?"), st.get("materials", 0), st.get("textures", 0)))
        except Exception:
            self.status.setText("NEXORA desktop not reachable — is it running?")
            return
        self.mat_list.clear()
        self.tex_list.clear()
        try:
            for m in CLIENT.materials():
                it = QtWidgets.QListWidgetItem("%s   ·   %s" % (m.get("name"), m.get("category") or ""))
                it.setData(QtCore.Qt.UserRole, m)
                self.mat_list.addItem(it)
            for t in CLIENT.textures():
                it = QtWidgets.QListWidgetItem("%s   ·   %s" % (t.get("name"), t.get("map_type") or "texture"))
                it.setData(QtCore.Qt.UserRole, t)
                self.tex_list.addItem(it)
        except Exception as exc:
            self.status.setText("Error loading library: %s" % exc)

    def _filter(self, text):
        text = (text or "").lower()
        for lst in (self.mat_list, self.tex_list):
            for i in range(lst.count()):
                item = lst.item(i)
                item.setHidden(text not in item.text().lower())

    def _apply_current(self):
        lst = self.mat_list if self.tabs.currentIndex() == 0 else self.tex_list
        item = lst.currentItem()
        if not item:
            return
        if self.tabs.currentIndex() == 0:
            apply_material(item.data(QtCore.Qt.UserRole))
        else:
            apply_texture(item.data(QtCore.Qt.UserRole))


def open_library(*_):
    global _WINDOW
    if _WINDOW is None:
        _WINDOW = NexoraBrowser()
    _WINDOW.show()
    _WINDOW.raise_()
    _WINDOW.refresh()


# ---------------------------------------------------------------------------
# The Maya command (a real registered command → `cmds.nexoraLibrary()`)
# ---------------------------------------------------------------------------
class NexoraCommand(om.MPxCommand):
    NAME = COMMAND_NAME

    def __init__(self):
        super(NexoraCommand, self).__init__()

    def doIt(self, args):
        open_library()


def _command_creator():
    return NexoraCommand()


# ---------------------------------------------------------------------------
# Menu + shelf (spec §33)
# ---------------------------------------------------------------------------
def _build_menu():
    if cmds.menu(MENU_NAME, exists=True):
        cmds.deleteUI(MENU_NAME, menu=True)
    try:
        gmain = mel.eval("$tmp = $gMainWindow")
    except Exception:
        gmain = "MayaWindow"
    cmds.menu(MENU_NAME, parent=gmain, label="NEXORA", tearOff=True)
    cmds.menuItem(parent=MENU_NAME, label="Open NEXORA Library", command=open_library)
    cmds.menuItem(parent=MENU_NAME, divider=True)
    cmds.menuItem(parent=MENU_NAME, label="Apply Selected Material",
                  command=lambda *_: _apply_from_window(0))
    cmds.menuItem(parent=MENU_NAME, label="Import Selected Texture",
                  command=lambda *_: _apply_from_window(1))
    cmds.menuItem(parent=MENU_NAME, label="Capture Material from Selection",
                  command=lambda *_: capture_selected_material())
    cmds.menuItem(parent=MENU_NAME, label="Scan Scene", command=lambda *_: scan_scene())
    cmds.menuItem(parent=MENU_NAME, divider=True)
    cmds.menuItem(parent=MENU_NAME, label="Reconnect", command=lambda *_: CLIENT.reload_config())


def _apply_from_window(tab):
    open_library()
    if _WINDOW:
        _WINDOW.tabs.setCurrentIndex(tab)
        _WINDOW._apply_current()


def _build_shelf():
    # The shelf tab bar lives under the MEL global $gShelfTopLevel.
    try:
        top = mel.eval("$tmp = $gShelfTopLevel")
    except Exception:
        return
    if cmds.shelfLayout(SHELF_NAME, exists=True):
        cmds.deleteUI(SHELF_NAME)
    cmds.shelfLayout(SHELF_NAME, parent=top)
    btns = [
        ("Library", "Open NEXORA Library", "menuIconWindow.png",
         "import nexora_bridge; nexora_bridge.open_library()"),
        ("Capture", "Capture material from selection", "out_shadingEngine.png",
         "import nexora_bridge; nexora_bridge.capture_selected_material()"),
        ("Scan", "Scan scene textures", "menuIconFile.png",
         "import nexora_bridge; nexora_bridge.scan_scene()"),
    ]
    for label, ann, image, cmd in btns:
        try:
            cmds.shelfButton(parent=SHELF_NAME, label=label, annotation=ann,
                             imageOverlayLabel=label, image=image, command=cmd,
                             sourceType="python")
        except Exception:
            pass


def _remove_ui():
    if cmds.menu(MENU_NAME, exists=True):
        cmds.deleteUI(MENU_NAME, menu=True)
    if cmds.shelfLayout(SHELF_NAME, exists=True):
        cmds.deleteUI(SHELF_NAME)
    global _WINDOW
    if _WINDOW is not None:
        try:
            _WINDOW.close()
            _WINDOW.deleteLater()
        except Exception:
            pass
        _WINDOW = None


# ---------------------------------------------------------------------------
# Plug-in entry points (Maya Python API 2.0)
# ---------------------------------------------------------------------------
def initializePlugin(plugin):
    plugin_fn = om.MFnPlugin(plugin, "FRIXXY", PLUGIN_VERSION)
    try:
        plugin_fn.registerCommand(NexoraCommand.NAME, _command_creator)
    except Exception:
        om.MGlobal.displayError("NEXORA: failed to register command '%s'." % NexoraCommand.NAME)
        raise

    # Build UI + start the poller once Maya's UI is idle/ready.
    def _setup():
        try:
            _build_menu()
            _build_shelf()
            _start_poller()
            om.MGlobal.displayInfo("%s %s loaded." % (PLUGIN_NAME, PLUGIN_VERSION))
        except Exception as exc:
            om.MGlobal.displayError("NEXORA: init failed (%s)." % exc)

    maya.utils.executeDeferred(_setup)


def uninitializePlugin(plugin):
    _stop_poller()
    _remove_ui()
    plugin_fn = om.MFnPlugin(plugin)
    try:
        plugin_fn.deregisterCommand(NexoraCommand.NAME)
    except Exception:
        pass
    om.MGlobal.displayInfo("%s unloaded." % PLUGIN_NAME)
