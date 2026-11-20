# Launcher installation

The MSI installs Open3D Launcher for the current user only, with no administrator rights, by
default into `%LOCALAPPDATA%\Programs\Open3D\Launcher`. It records the folder and the
desktop-shortcut choice under `HKCU\Software\Open3D\Open3D Launcher`, so an update (the
launcher's self-update runs the new MSI with `/passive`) lands in the same folder and keeps the
choice; an explicit `INSTALLFOLDER` on the command line wins. Staying per-user is what lets the
launcher update itself without elevation.

The portable zip needs no installer: `portable.txt` next to `open3d-launcher.exe` keeps the
settings and download cache in `Open3DLauncherData\` beside it.

The apps are of two kinds. The 3ds Max plug-ins are bundles: the launcher unpacks them into
`%APPDATA%\Autodesk\ApplicationPlugins\<App>.bundle`, where every 3ds Max version loads them, with no
administrator rights. The programs (OpenView, OpenRef, OpenFarm, OpenGaussian) are installed for
everyone on the computer by their own per-machine MSI, into `C:\Program Files\Open3D\<App>`, when their
release has one: the launcher starts `msiexec` as any program, so Windows asks for permission at each
install, update and removal. A release without one, and a portable launcher always, unpacks the
program's zip instead, per user, into `%LOCALAPPDATA%\Programs\Open3D\<App>` (a portable launcher: into
`Open3DLauncherData\Programs\<App>`).

`./packaging/windows/test-install-policy.ps1 -OutputDirectory ./target/install-policy-tests`
builds non-executable MSIs for x64, x86 and arm64 and checks their compiled paths, identity and
registration (`check-install-policy.ps1`). Never install them. `package.ps1` runs the same check
on the real MSI.
