# Deploying the Excel add-in

For whoever installs the add-in on a desk. The add-in itself is described in
[excel/README.md](../excel/README.md).

It needs Microsoft 365 Apps for Windows, 64-bit, on the Current or Monthly
Enterprise channel, and .NET Framework 4.7.2. Excel 2016 and 2019 are not
supported, because the grid functions need dynamic arrays.

## Install

Each [GitHub release](https://github.com/sujitn/convex/releases) has
`convex-excel-addin-v<version>.msi`. Close Excel, then run it. It installs for
the current user and needs no administrator rights. To run it silently:

```powershell
msiexec /i convex-excel-addin-v<version>.msi /qn /l*v install.log
```

The installer:

- puts `Convex.Excel64.xll` and `convex_ffi.dll` in
  `%LOCALAPPDATA%\Convex\Excel\<version>\`;
- adds **Convex Excel Add-in** to Apps & features;
- adds one value to `HKCU\Software\Microsoft\Office\16.0\Excel\Options`, so
  Excel loads the add-in when it starts. It is the next free name in the series
  `OPEN`, `OPEN1`, `OPEN2` and so on.

The installer refuses to run while any Excel process is open, because Excel
holds the add-in open and may rewrite that list when it exits. A silent run then
exits with code 1603. On a shared host, another user's Excel counts too.

If the add-in was loaded by hand earlier, remove it first in File > Options >
Add-ins, or Excel loads both copies.

## Upgrade and uninstall

Run a newer MSI to upgrade. It removes the old version's folder and points the
Excel entry at the new one. If the upgrade fails, the old version is put back.
An older MSI is refused, so uninstall first to go back to an earlier version.

To uninstall, close Excel and use Apps & features, or:

```powershell
msiexec /x convex-excel-addin-v<version>.msi /qn
```

This removes the version's folder and the Excel entry, and moves any later
add-ins in the list down one place. The logs in
`%LOCALAPPDATA%\Convex\Excel\logs` and the server settings in
`%APPDATA%\Convex\server.json` stay.

## Signing and trust

**The installer and the add-in are not code-signed yet.** Until they are:

- Windows SmartScreen may warn when the MSI is run from a download.
- A desk whose policy requires add-ins from a trusted publisher cannot load the
  add-in.

Files an MSI installs carry no Mark of the Web, so Excel does not block the
installed add-in as a download and nothing needs unblocking. The zip on the
release page does need unblocking; its `README.txt` has the steps.

## Intune and Group Policy

The MSI is per-user, so deploy it in the user's context: in Intune, as a line-of-business
app with the install context set to User; in Group Policy, under User
Configuration. It takes no properties. Plan for the 1603 exit code when Excel is open.

So far the MSI has been tested with `msiexec` only, not through Intune or
Group Policy.

## Live cell update rate

Excel delivers changed live cells at most once per `RTD.ThrottleInterval`,
2,000 ms by default. The add-in never changes this by itself: the setting
belongs to Excel, applies to every add-in's live cells and persists after Excel
restarts. Settings > "Excel RTD throttle" offers 250 ms. Very low values can make
Excel sluggish.

## Other add-ins

The installer changes only its own entry in the `OPEN` list. Other add-ins keep
their order. The add-in runs inside Excel's process alongside them.

## Logs

The log file is `%LOCALAPPDATA%\Convex\Excel\logs\convex-excel.log`. A second
Excel process writes `convex-excel-<pid>.log`. `=CX.DIAG()` shows what loaded and the
state of the server link. The ribbon's **Error Log** shows each cell's latest
error and the recent connection events.

## Windows on ARM

Office on Windows on ARM can load 64-bit x64 add-ins, so this one should work.
It has not been tested there.

## For maintainers: turning on signing

Signing happens in `excel/installer/Sign.ps1`, after packing: first the `.xll`
and the DLL, then the MSI that contains them. The release workflow signs when
the repository variable `EXCEL_SIGN_THUMBPRINT` names a certificate available to
the runner, and uses `EXCEL_SIGN_TIMESTAMP_URL` as the timestamp server. A
publicly trusted key must be held in hardware, so a CA's cloud HSM client has to
be set up on the runner first. With no variable set, the release is unsigned and
its notes say so.
