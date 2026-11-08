; Retire l'entrée de démarrage automatique (écrite par tauri-plugin-autostart, nommée d'après productName).
; Les données (dossiers de données d'Oniris) sont conservées volontairement.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Oniris"
!macroend
