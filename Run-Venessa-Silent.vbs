Set WshShell = CreateObject("WScript.Shell")
WshShell.Run chr(34) & "target\release\venessa.exe" & chr(34), 0
Set WshShell = Nothing
