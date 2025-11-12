# Set JAVA_HOME system environment variable
# Run this script as Administrator

$javaPath = "C:\Program Files\Microsoft\jdk-17.0.17.10-hotspot"

Write-Host "Setting JAVA_HOME to: $javaPath"

# Set system-wide environment variable (requires admin)
[System.Environment]::SetEnvironmentVariable('JAVA_HOME', $javaPath, 'Machine')

# Also add to PATH if not already there
$currentPath = [System.Environment]::GetEnvironmentVariable('Path', 'Machine')
$javaBinPath = "$javaPath\bin"

if ($currentPath -notlike "*$javaBinPath*") {
    Write-Host "Adding Java bin to PATH: $javaBinPath"
    $newPath = "$currentPath;$javaBinPath"
    [System.Environment]::SetEnvironmentVariable('Path', $newPath, 'Machine')
} else {
    Write-Host "Java bin already in PATH"
}

Write-Host ""
Write-Host "✓ JAVA_HOME set successfully!"
Write-Host "  JAVA_HOME = $javaPath"
Write-Host ""
Write-Host "Please restart your terminal/IDE for changes to take effect."
Write-Host ""
Write-Host "To verify, open a NEW terminal and run:"
Write-Host '  echo $env:JAVA_HOME'
Write-Host '  java -version'
