; Preserve login startup during /UPDATE. A normal uninstall removes only the
; command owned by this installation, leaving another installation untouched.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Push $0
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SubGauge.Desktop"
    StrCmp $0 '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --autostart' 0 +2
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SubGauge.Desktop"
    Pop $0
  ${EndIf}
!macroend
