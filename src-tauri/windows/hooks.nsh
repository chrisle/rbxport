!macro NSIS_HOOK_POSTINSTALL
  ; Keep the elevated entry point under Program Files. The scheduled task
  ; never executes an updater binary from a user-writable directory.
  CopyFiles /SILENT "$INSTDIR\rbxport.exe" "$INSTDIR\rbxport-updater.exe"

  ; The script creates the protected shared staging directory and an on-demand
  ; SYSTEM task. It also gives authenticated users execute-only access to the
  ; task; the helper independently verifies every staged installer signature.
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\install-update-task.ps1" -InstallDirectory "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "Could not install the silent update task (exit $0): $1"
  ${EndIf}
  Delete /REBOOTOK "$INSTDIR\rbxport-update-installer.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove the privileged entry point before the ordinary uninstaller removes
  ; the files it knows about. The helper copy itself is generated at install
  ; time, so it is not in Tauri's resource manifest.
  ${If} ${FileExists} "$INSTDIR\install-update-task.ps1"
    nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\install-update-task.ps1" -InstallDirectory "$INSTDIR" -Uninstall'
    Pop $0
  ${EndIf}
  Delete /REBOOTOK "$INSTDIR\rbxport-updater.exe"
  Delete /REBOOTOK "$INSTDIR\rbxport-update-installer.exe"
!macroend
