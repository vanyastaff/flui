#!/usr/bin/env pwsh
# Create a new feature using git worktree for isolation
[CmdletBinding()]
param(
    [switch]$Json,
    [string]$ShortName,
    [int]$Number = 0,
    [switch]$Help,
    [string]$WorktreeDir,  # Custom worktree directory (default: ../.worktrees/<branch-name>)
    [switch]$NoWorktree,   # Use old checkout behavior instead of worktree
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$FeatureDescription
)
$ErrorActionPreference = 'Stop'

# Show help if requested
if ($Help)
{
    Write-Host "Usage: ./create-new-feature.ps1 [-Json] [-ShortName <name>] [-Number N] [-WorktreeDir <path>] [-NoWorktree] <feature description>"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Json                Output in JSON format"
    Write-Host "  -ShortName <name>    Provide a custom short name (2-4 words) for the branch"
    Write-Host "  -Number N            Specify branch number manually (overrides auto-detection)"
    Write-Host "  -WorktreeDir <path>  Custom worktree directory (default: ../.worktrees/<branch-name>)"
    Write-Host "  -NoWorktree          Use git checkout instead of worktree (legacy mode)"
    Write-Host "  -Help                Show this help message"
    Write-Host ""
    Write-Host "Git Worktree Benefits:"
    Write-Host "  - Each feature in isolated directory (no context switching)"
    Write-Host "  - Work on multiple features simultaneously"
    Write-Host "  - Safe: never lose uncommitted work from branch switching"
    Write-Host "  - Clean: specs/ directory stays in sync with worktree"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  ./create-new-feature.ps1 'Add user authentication system' -ShortName 'user-auth'"
    Write-Host "  ./create-new-feature.ps1 'Implement OAuth2 integration for API'"
    Write-Host "  ./create-new-feature.ps1 'Fix bug in parser' -WorktreeDir 'C:/dev/flui-features/parser-fix'"
    exit 0
}

# Check if feature description provided
if (-not $FeatureDescription -or $FeatureDescription.Count -eq 0)
{
    Write-Error "Usage: ./create-new-feature.ps1 [-Json] [-ShortName <name>] <feature description>"
    exit 1
}

$featureDesc = ($FeatureDescription -join ' ').Trim()

# Resolve repository root. Prefer git information when available, but fall back
# to searching for repository markers so the workflow still functions in repositories that
# were initialized with --no-git.
function Find-RepositoryRoot
{
    param(
        [string]$StartDir,
        [string[]]$Markers = @('.git', '.specify')
    )
    $current = Resolve-Path $StartDir
    while ($true)
    {
        foreach ($marker in $Markers)
        {
            if (Test-Path (Join-Path $current $marker))
            {
                return $current
            }
        }
        $parent = Split-Path $current -Parent
        if ($parent -eq $current)
        {
            # Reached filesystem root without finding markers
            return $null
        }
        $current = $parent
    }
}

function Get-HighestNumberFromSpecs
{
    param([string]$SpecsDir)

    $highest = 0
    if (Test-Path $SpecsDir)
    {
        Get-ChildItem -Path $SpecsDir -Directory | ForEach-Object {
            if ($_.Name -match '^(\d+)')
            {
                $num = [int]$matches[1]
                if ($num -gt $highest)
                { $highest = $num
                }
            }
        }
    }
    return $highest
}

function Get-HighestNumberFromBranches
{
    param()

    $highest = 0
    try
    {
        $branches = git branch -a 2>$null
        if ($LASTEXITCODE -eq 0)
        {
            foreach ($branch in $branches)
            {
                # Clean branch name: remove leading markers and remote prefixes
                $cleanBranch = $branch.Trim() -replace '^\*?\s+', '' -replace '^remotes/[^/]+/', ''

                # Extract feature number if branch matches pattern ###-*
                if ($cleanBranch -match '^(\d+)-')
                {
                    $num = [int]$matches[1]
                    if ($num -gt $highest)
                    { $highest = $num
                    }
                }
            }
        }
    } catch
    {
        # If git command fails, return 0
        Write-Verbose "Could not check Git branches: $_"
    }
    return $highest
}

function Get-NextBranchNumber
{
    param(
        [string]$SpecsDir
    )

    # Fetch all remotes to get latest branch info (suppress errors if no remotes)
    try
    {
        git fetch --all --prune 2>$null | Out-Null
    } catch
    {
        # Ignore fetch errors
    }

    # Get highest number from ALL branches (not just matching short name)
    $highestBranch = Get-HighestNumberFromBranches

    # Get highest number from ALL specs (not just matching short name)
    $highestSpec = Get-HighestNumberFromSpecs -SpecsDir $SpecsDir

    # Take the maximum of both
    $maxNum = [Math]::Max($highestBranch, $highestSpec)

    # Return next number
    return $maxNum + 1
}

function ConvertTo-CleanBranchName
{
    param([string]$Name)

    return $Name.ToLower() -replace '[^a-z0-9]', '-' -replace '-{2,}', '-' -replace '^-', '' -replace '-$', ''
}

function Get-BranchName
{
    param([string]$Description)

    # Common stop words to filter out
    $stopWords = @(
        'i', 'a', 'an', 'the', 'to', 'for', 'of', 'in', 'on', 'at', 'by', 'with', 'from',
        'is', 'are', 'was', 'were', 'be', 'been', 'being', 'have', 'has', 'had',
        'do', 'does', 'did', 'will', 'would', 'should', 'could', 'can', 'may', 'might', 'must', 'shall',
        'this', 'that', 'these', 'those', 'my', 'your', 'our', 'their',
        'want', 'need', 'add', 'get', 'set'
    )

    # Convert to lowercase and extract words (alphanumeric only)
    $cleanName = $Description.ToLower() -replace '[^a-z0-9\s]', ' '
    $words = $cleanName -split '\s+' | Where-Object { $_ }

    # Filter words: remove stop words and words shorter than 3 chars (unless they're uppercase acronyms in original)
    $meaningfulWords = @()
    foreach ($word in $words)
    {
        # Skip stop words
        if ($stopWords -contains $word)
        { continue
        }

        # Keep words that are length >= 3 OR appear as uppercase in original (likely acronyms)
        if ($word.Length -ge 3)
        {
            $meaningfulWords += $word
        } elseif ($Description -match "\b$($word.ToUpper())\b")
        {
            # Keep short words if they appear as uppercase in original (likely acronyms)
            $meaningfulWords += $word
        }
    }

    # If we have meaningful words, use first 3-4 of them
    if ($meaningfulWords.Count -gt 0)
    {
        $maxWords = if ($meaningfulWords.Count -eq 4)
        { 4
        } else
        { 3
        }
        $result = ($meaningfulWords | Select-Object -First $maxWords) -join '-'
        return $result
    } else
    {
        # Fallback to original logic if no meaningful words found
        $result = ConvertTo-CleanBranchName -Name $Description
        $fallbackWords = ($result -split '-') | Where-Object { $_ } | Select-Object -First 3
        return [string]::Join('-', $fallbackWords)
    }
}

$fallbackRoot = (Find-RepositoryRoot -StartDir $PSScriptRoot)
if (-not $fallbackRoot)
{
    Write-Error "Error: Could not determine repository root. Please run this script from within the repository."
    exit 1
}

try
{
    $repoRoot = git rev-parse --show-toplevel 2>$null
    if ($LASTEXITCODE -eq 0)
    {
        $hasGit = $true
    } else
    {
        throw "Git not available"
    }
} catch
{
    $repoRoot = $fallbackRoot
    $hasGit = $false
}

Set-Location $repoRoot

$specsDir = Join-Path $repoRoot 'specs'
New-Item -ItemType Directory -Path $specsDir -Force | Out-Null

# Generate branch name
if ($ShortName)
{
    # Use provided short name, just clean it up
    $branchSuffix = ConvertTo-CleanBranchName -Name $ShortName
} else
{
    # Generate from description with smart filtering
    $branchSuffix = Get-BranchName -Description $featureDesc
}

# Determine branch number
if ($Number -eq 0)
{
    if ($hasGit)
    {
        # Check existing branches on remotes
        $Number = Get-NextBranchNumber -SpecsDir $specsDir
    } else
    {
        # Fall back to local directory check
        $Number = (Get-HighestNumberFromSpecs -SpecsDir $specsDir) + 1
    }
}

$featureNum = ('{0:000}' -f $Number)
$branchName = "$featureNum-$branchSuffix"

# GitHub enforces a 244-byte limit on branch names
# Validate and truncate if necessary
$maxBranchLength = 244
if ($branchName.Length -gt $maxBranchLength)
{
    # Calculate how much we need to trim from suffix
    # Account for: feature number (3) + hyphen (1) = 4 chars
    $maxSuffixLength = $maxBranchLength - 4

    # Truncate suffix
    $truncatedSuffix = $branchSuffix.Substring(0, [Math]::Min($branchSuffix.Length, $maxSuffixLength))
    # Remove trailing hyphen if truncation created one
    $truncatedSuffix = $truncatedSuffix -replace '-$', ''

    $originalBranchName = $branchName
    $branchName = "$featureNum-$truncatedSuffix"

    Write-Warning "[specify] Branch name exceeded GitHub's 244-byte limit"
    Write-Warning "[specify] Original: $originalBranchName ($($originalBranchName.Length) bytes)"
    Write-Warning "[specify] Truncated to: $branchName ($($branchName.Length) bytes)"
}

# Determine worktree path
$worktreePath = $null
$useWorktree = $hasGit -and (-not $NoWorktree)

if ($useWorktree)
{
    if ($WorktreeDir)
    {
        # User specified custom worktree directory
        $worktreePath = $WorktreeDir
    } else
    {
        # Default: ../.worktrees/<branch-name> (sibling to repo)
        $repoParent = Split-Path $repoRoot -Parent
        $worktreesBase = Join-Path $repoParent '.worktrees'
        $worktreePath = Join-Path $worktreesBase $branchName
    }

    # Ensure worktree path is absolute
    if (-not [System.IO.Path]::IsPathRooted($worktreePath))
    {
        $worktreePath = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $worktreePath))
    }
}

# Create branch and worktree/checkout
if ($hasGit)
{
    try
    {
        if ($useWorktree)
        {
            # Check if worktree path already exists
            if (Test-Path $worktreePath)
            {
                Write-Warning "[specify] Worktree directory already exists: $worktreePath"
                Write-Warning "[specify] Please remove it manually or choose a different location"
                exit 1
            }

            # Create parent directory if it doesn't exist
            $worktreeParent = Split-Path $worktreePath -Parent
            if (-not (Test-Path $worktreeParent))
            {
                New-Item -ItemType Directory -Path $worktreeParent -Force | Out-Null
                Write-Verbose "[specify] Created worktree parent directory: $worktreeParent"
            }

            # Create worktree with new branch
            Write-Host "[specify] Creating worktree at: $worktreePath"
            git worktree add -b $branchName $worktreePath 2>&1 | Out-Null

            if ($LASTEXITCODE -ne 0)
            {
                Write-Error "[specify] Failed to create git worktree for branch: $branchName"
                exit 1
            }

            Write-Host "[specify] ✓ Worktree created successfully"
            Write-Host "[specify] ✓ Branch: $branchName"
            Write-Host "[specify] ✓ Location: $worktreePath"
            Write-Host ""
            Write-Host "[specify] To work on this feature:"
            Write-Host "  cd `"$worktreePath`""
            Write-Host ""
            Write-Host "[specify] Tip: Your main worktree remains untouched, so you can keep working there too!"

        } else
        {
            # Legacy mode: use checkout
            git checkout -b $branchName 2>&1 | Out-Null
            if ($LASTEXITCODE -ne 0)
            {
                Write-Warning "[specify] Failed to create git branch: $branchName"
            } else
            {
                Write-Host "[specify] ✓ Branch created: $branchName"
            }
        }
    } catch
    {
        Write-Warning "[specify] Git operation failed: $_"
    }
} else
{
    Write-Warning "[specify] Warning: Git repository not detected; skipped branch creation for $branchName"
}

# Create specs directory structure
# For worktrees, create in the worktree location; otherwise in main repo
if ($useWorktree -and $worktreePath)
{
    $targetSpecsDir = Join-Path $worktreePath 'specs'
} else
{
    $targetSpecsDir = $specsDir
}

$featureDir = Join-Path $targetSpecsDir $branchName
New-Item -ItemType Directory -Path $featureDir -Force | Out-Null

# Copy spec template
$template = Join-Path $repoRoot '.specify/templates/spec-template.md'
$specFile = Join-Path $featureDir 'spec.md'

if ($useWorktree -and $worktreePath)
{
    # For worktrees, check template in both main repo and worktree
    $worktreeTemplate = Join-Path $worktreePath '.specify/templates/spec-template.md'
    if (Test-Path $worktreeTemplate)
    {
        Copy-Item $worktreeTemplate $specFile -Force
    } elseif (Test-Path $template)
    {
        Copy-Item $template $specFile -Force
    } else
    {
        New-Item -ItemType File -Path $specFile | Out-Null
    }
} else
{
    if (Test-Path $template)
    {
        Copy-Item $template $specFile -Force
    } else
    {
        New-Item -ItemType File -Path $specFile | Out-Null
    }
}

# Set the SPECIFY_FEATURE environment variable for the current session
$env:SPECIFY_FEATURE = $branchName

# Output results
if ($Json)
{
    $obj = [PSCustomObject]@{
        BRANCH_NAME = $branchName
        SPEC_FILE = $specFile
        FEATURE_NUM = $featureNum
        HAS_GIT = $hasGit
        WORKTREE_PATH = $worktreePath
        USE_WORKTREE = $useWorktree
    }
    $obj | ConvertTo-Json -Compress
} else
{
    Write-Output "BRANCH_NAME: $branchName"
    Write-Output "SPEC_FILE: $specFile"
    Write-Output "FEATURE_NUM: $featureNum"
    Write-Output "HAS_GIT: $hasGit"
    if ($useWorktree)
    {
        Write-Output "WORKTREE_PATH: $worktreePath"
        Write-Output "USE_WORKTREE: true"
    } else
    {
        Write-Output "USE_WORKTREE: false"
    }
    Write-Output "SPECIFY_FEATURE environment variable set to: $branchName"
}
