param(
    [Parameter(Mandatory = $true)]
    [string] $InstallDirectory,
    [switch] $Uninstall,
    [switch] $ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$taskName = "rbxport Silent Update"
$stagingDirectory = Join-Path $env:ProgramData "rbxport\updates"

if ($ValidateOnly) {
    if ([string]::IsNullOrWhiteSpace($InstallDirectory)) {
        throw "InstallDirectory is required."
    }
    exit 0
}

if ($Uninstall) {
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stagingDirectory -Recurse -Force -ErrorAction SilentlyContinue
    exit 0
}

$helper = Join-Path $InstallDirectory "rbxport-updater.exe"
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
    throw "The protected update helper is missing: $helper"
}

# Users may stage candidate bytes here, but only the SYSTEM task can copy them
# into Program Files. The helper verifies the updater's minisign signature and
# target version before that copy, so write access does not grant elevation.
New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null
$acl = New-Object System.Security.AccessControl.DirectorySecurity
$acl.SetAccessRuleProtection($true, $false)
foreach ($rule in @(
    New-Object System.Security.AccessControl.FileSystemAccessRule(
        "SYSTEM", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow"
    ),
    New-Object System.Security.AccessControl.FileSystemAccessRule(
        "BUILTIN\Administrators", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow"
    ),
    New-Object System.Security.AccessControl.FileSystemAccessRule(
        "NT AUTHORITY\Authenticated Users", "Modify", "ContainerInherit,ObjectInherit", "None", "Allow"
    )
)) {
    $acl.AddAccessRule($rule)
}
Set-Acl -LiteralPath $stagingDirectory -AclObject $acl

$action = New-ScheduledTaskAction -Execute $helper -Argument "--apply-staged-update"
$principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero)
$task = New-ScheduledTask -Action $action -Principal $principal -Settings $settings `
    -Description "Installs a signed rbxport update after the app exits."
Register-ScheduledTask -TaskName $taskName -InputObject $task -Force | Out-Null

# Register-ScheduledTask creates an administrator-only task. Authenticated
# users need read/execute rights so a non-elevated rbxport process can start
# it, while only administrators and SYSTEM may change or delete it.
$scheduler = New-Object -ComObject "Schedule.Service"
$scheduler.Connect()
$registeredTask = $scheduler.GetFolder("\").GetTask($taskName)
$registeredTask.SetSecurityDescriptor(
    "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;AU)",
    0
)
