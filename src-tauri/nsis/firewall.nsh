; Inbound rules for the two sockets discovery and pairing need.
;
; Discovery is UDP broadcast to the well-known port; pairing is a TCP dial to
; the listener port the announcement carries. Windows blocks both for a new
; program until a rule exists, and a blocked discovery makes every other
; network feature look broken while the app itself starts normally.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Synra"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Synra" dir=in action=allow program="$INSTDIR\synra-app.exe" enable=yes profile=private,domain'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Synra"'
!macroend