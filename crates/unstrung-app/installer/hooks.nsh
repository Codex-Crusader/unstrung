; Start menu shortcut for safe start (see safe.rs), next to the main one.
!macro NSIS_HOOK_POSTINSTALL
  CreateShortCut "$SMPROGRAMS\${PRODUCTNAME} (Safe start).lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--safe-start"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Delete "$SMPROGRAMS\${PRODUCTNAME} (Safe start).lnk"
  ${EndIf}
!macroend
