; NEXORA — install the Maya plug-in as part of the app installer.
;
; Tauri runs these hooks inside its generated NSIS installer. After the app
; files are laid down, we copy the bundled Maya plug-in into the per-user Maya
; plug-ins folders for 2026 and 2027 so it's ready to enable in Maya's Plug-in
; Manager. The Python plug-in (nexora_bridge.py) is always bundled; a compiled
; nexora_bridge.mll is copied too when one was placed under
; plugins/maya/prebuilt/<version>/ before building (see that folder's README).
;
; Bundled resources live under the install dir; the exact subfolder has varied
; across Tauri versions, so we probe the known candidates.

!macro NSIS_HOOK_POSTINSTALL
  ; Find where the bundled plug-in landed.
  StrCpy $R0 ""
  IfFileExists "$INSTDIR\resources\maya-plugin\nexora_bridge.py" 0 +2
    StrCpy $R0 "$INSTDIR\resources\maya-plugin"
  IfFileExists "$INSTDIR\maya-plugin\nexora_bridge.py" 0 +2
    StrCpy $R0 "$INSTDIR\maya-plugin"

  ; If we couldn't locate it, skip quietly rather than fail the install.
  StrCmp $R0 "" nexora_plugin_done

  ; --- Maya 2026 ---
  CreateDirectory "$DOCUMENTS\maya\2026\plug-ins"
  CopyFiles /SILENT "$R0\nexora_bridge.py" "$DOCUMENTS\maya\2026\plug-ins\nexora_bridge.py"
  IfFileExists "$R0\2026\nexora_bridge.mll" 0 +2
    CopyFiles /SILENT "$R0\2026\nexora_bridge.mll" "$DOCUMENTS\maya\2026\plug-ins\nexora_bridge.mll"

  ; --- Maya 2027 ---
  CreateDirectory "$DOCUMENTS\maya\2027\plug-ins"
  CopyFiles /SILENT "$R0\nexora_bridge.py" "$DOCUMENTS\maya\2027\plug-ins\nexora_bridge.py"
  IfFileExists "$R0\2027\nexora_bridge.mll" 0 +2
    CopyFiles /SILENT "$R0\2027\nexora_bridge.mll" "$DOCUMENTS\maya\2027\plug-ins\nexora_bridge.mll"

  nexora_plugin_done:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the plug-in files we installed (leave any user data alone).
  Delete "$DOCUMENTS\maya\2026\plug-ins\nexora_bridge.py"
  Delete "$DOCUMENTS\maya\2026\plug-ins\nexora_bridge.mll"
  Delete "$DOCUMENTS\maya\2027\plug-ins\nexora_bridge.py"
  Delete "$DOCUMENTS\maya\2027\plug-ins\nexora_bridge.mll"
!macroend
