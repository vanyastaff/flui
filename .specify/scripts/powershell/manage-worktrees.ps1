#!/usr/bin/env pwsh
# Manage git worktrees for features
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('list', 'remove', 'prune', 'clean', 'help')]
    [string]$Command = 'list',

    [Parameter(Position = 1)]
    [string]$BranchName,

    [switch]$Force,
    [switch]$Json,
    [switch]$All
)
$ErrorActionPreference = 'Stop'

function Show-Help
{
    Write-Host "Git Worktree Manager for Speckit Features"
    Write-Host ""
    Write-Host "Usage: ./manage-worktrees.ps1 <command> [options]"
    Write-Host ""
    Write-Host "Commands:"
    Write-Host "  list                 List all worktrees (default)"
    Write-Host "  remove <branch>      Remove specific worktree"
    Write-Host "  prune                Remove stale worktree references"
    Write-Host "  clean                Remove all feature worktrees (requires -Force)"
    Write-Host "  help                 Show this help message"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Force               Force removal without confirmation"
    Write-Host "  -Json                Output in JSON format"
    Write-Host "  -All                 Include main worktree in operations"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  ./manage-worktrees.ps1 list"
    Write-Host "  ./manage-worktrees.ps1 remove 001-user-auth"
    Write-Host "  ./manage-worktrees.ps1 prune"
    Write-Host "  ./manage-worktrees.ps1 clean -Force"
}

function Get-Worktrees
{
    $worktrees = @()

    try
    {
        $output = git worktree list --porcelain 2>$null
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "Failed to list worktrees. Make sure you're in a git repository."
            return $worktrees
        }

        $current = @{}
        foreach ($line in $output -split "`n")
        {
            $line = $line.Trim()

            if ($line -match '^worktree (.+)$')
            {
                if ($current.Count -gt 0)
                {
                    $worktrees += [PSCustomObject]$current
                }
                $current = @{
                    Path = $matches[1]
                    Branch = $null
                    Commit = $null
                    IsMain = $false
                    IsBare = $false
                    IsDetached = $false
                }
            } elseif ($line -match '^HEAD (.+)$')
            {
                $current.Commit = $matches[1]
            } elseif ($line -match '^branch (.+)$')
            {
                $current.Branch = $matches[1] -replace '^refs/heads/', ''
            } elseif ($line -eq 'bare')
            {
                $current.IsBare = $true
            } elseif ($line -eq 'detached')
            {
                $current.IsDetached = $true
            }
        }

        # Add last worktree
        if ($current.Count -gt 0)
        {
            $worktrees += [PSCustomObject]$current
        }

        # Mark main worktree (first one is always main)
        if ($worktrees.Count -gt 0)
        {
            $worktrees[0].IsMain = $true
        }

    } catch
    {
        Write-Error "Error listing worktrees: $_"
    }

    return $worktrees
}

function Format-WorktreeList
{
    param([array]$Worktrees)

    if ($Worktrees.Count -eq 0)
    {
        Write-Host "[specify] No worktrees found."
        return
    }

    Write-Host ""
    Write-Host "Git Worktrees:" -ForegroundColor Cyan
    Write-Host ("=" * 80)

    foreach ($wt in $Worktrees)
    {
        $marker = if ($wt.IsMain)
        { "[MAIN]" 
        } else
        { "      " 
        }
        $branch = if ($wt.Branch)
        { $wt.Branch 
        } elseif ($wt.IsDetached)
        { "DETACHED" 
        } else
        { "N/A" 
        }
        $commit = $wt.Commit.Substring(0, [Math]::Min(8, $wt.Commit.Length))

        Write-Host "$marker " -NoNewline -ForegroundColor $(if ($wt.IsMain)
            { 'Yellow' 
            } else
            { 'Gray' 
            })
        Write-Host "$branch " -NoNewline -ForegroundColor $(if ($wt.IsMain)
            { 'Green' 
            } else
            { 'White' 
            })
        Write-Host "($commit)" -ForegroundColor DarkGray
        Write-Host "       $($wt.Path)" -ForegroundColor Gray

        if ($wt.IsBare)
        {
            Write-Host "       [bare repository]" -ForegroundColor DarkYellow
        }
        Write-Host ""
    }
}

function Remove-Worktree
{
    param(
        [string]$Branch,
        [switch]$Force
    )

    $worktrees = Get-Worktrees
    $target = $worktrees | Where-Object { $_.Branch -eq $Branch -and -not $_.IsMain }

    if (-not $target)
    {
        Write-Error "Worktree for branch '$Branch' not found (or it's the main worktree)."
        return $false
    }

    if (-not $Force)
    {
        $response = Read-Host "Remove worktree for branch '$Branch' at $($target.Path)? (y/N)"
        if ($response -ne 'y' -and $response -ne 'Y')
        {
            Write-Host "Cancelled."
            return $false
        }
    }

    try
    {
        Write-Host "[specify] Removing worktree: $Branch"

        # Check if worktree has uncommitted changes
        Push-Location $target.Path
        $status = git status --porcelain 2>$null
        Pop-Location

        if ($status -and -not $Force)
        {
            Write-Warning "Worktree has uncommitted changes. Use -Force to remove anyway."
            Write-Host "Changes:"
            $status | ForEach-Object { Write-Host "  $_" }
            return $false
        }

        # Remove worktree
        $forceFlag = if ($Force)
        { '--force' 
        } else
        { '' 
        }
        git worktree remove $forceFlag $target.Path 2>&1 | Out-Null

        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "Failed to remove worktree."
            return $false
        }

        Write-Host "[specify] ✓ Worktree removed successfully"

        # Optionally delete the branch
        $deleteBranch = Read-Host "Delete branch '$Branch' as well? (y/N)"
        if ($deleteBranch -eq 'y' -or $deleteBranch -eq 'Y')
        {
            git branch -d $Branch 2>&1 | Out-Null
            if ($LASTEXITCODE -eq 0)
            {
                Write-Host "[specify] ✓ Branch deleted successfully"
            } else
            {
                Write-Warning "Failed to delete branch. It may have unmerged changes."
                $forceDelete = Read-Host "Force delete? This will lose unmerged changes. (y/N)"
                if ($forceDelete -eq 'y' -or $forceDelete -eq 'Y')
                {
                    git branch -D $Branch 2>&1 | Out-Null
                    Write-Host "[specify] ✓ Branch force-deleted"
                }
            }
        }

        return $true

    } catch
    {
        Write-Error "Error removing worktree: $_"
        return $false
    }
}

function Invoke-PruneWorktrees
{
    Write-Host "[specify] Pruning stale worktree references..."

    try
    {
        git worktree prune 2>&1 | Out-Null

        if ($LASTEXITCODE -eq 0)
        {
            Write-Host "[specify] ✓ Worktree references pruned successfully"
            return $true
        } else
        {
            Write-Error "Failed to prune worktrees."
            return $false
        }

    } catch
    {
        Write-Error "Error pruning worktrees: $_"
        return $false
    }
}

function Invoke-CleanWorktrees
{
    param([switch]$Force)

    if (-not $Force)
    {
        Write-Error "Clean operation requires -Force flag to prevent accidental deletion."
        Write-Host "This will remove ALL feature worktrees (except main)."
        Write-Host "Use: ./manage-worktrees.ps1 clean -Force"
        return $false
    }

    $worktrees = Get-Worktrees
    $featureWorktrees = $worktrees | Where-Object { -not $_.IsMain }

    if ($featureWorktrees.Count -eq 0)
    {
        Write-Host "[specify] No feature worktrees to clean."
        return $true
    }

    Write-Host "[specify] Found $($featureWorktrees.Count) feature worktree(s) to remove:"
    foreach ($wt in $featureWorktrees)
    {
        Write-Host "  - $($wt.Branch) at $($wt.Path)"
    }
    Write-Host ""

    $response = Read-Host "Proceed with removal? (yes/N)"
    if ($response -ne 'yes')
    {
        Write-Host "Cancelled."
        return $false
    }

    $successCount = 0
    $failCount = 0

    foreach ($wt in $featureWorktrees)
    {
        Write-Host "[specify] Removing: $($wt.Branch)..."

        try
        {
            git worktree remove --force $wt.Path 2>&1 | Out-Null

            if ($LASTEXITCODE -eq 0)
            {
                $successCount++
                Write-Host "[specify] ✓ Removed: $($wt.Branch)" -ForegroundColor Green
            } else
            {
                $failCount++
                Write-Warning "Failed to remove: $($wt.Branch)"
            }

        } catch
        {
            $failCount++
            Write-Warning "Error removing $($wt.Branch): $_"
        }
    }

    Write-Host ""
    Write-Host "[specify] Clean complete: $successCount removed, $failCount failed"

    if ($failCount -eq 0)
    {
        # Prune stale references
        Invoke-PruneWorktrees | Out-Null
    }

    return ($failCount -eq 0)
}

# Main command execution
switch ($Command)
{
    'help'
    {
        Show-Help
    }

    'list'
    {
        $worktrees = Get-Worktrees

        if ($Json)
        {
            $worktrees | ConvertTo-Json
        } else
        {
            Format-WorktreeList -Worktrees $worktrees
        }
    }

    'remove'
    {
        if (-not $BranchName)
        {
            Write-Error "Branch name required for remove command."
            Write-Host "Usage: ./manage-worktrees.ps1 remove <branch-name> [-Force]"
            exit 1
        }

        $success = Remove-Worktree -Branch $BranchName -Force:$Force
        exit $(if ($success)
            { 0 
            } else
            { 1 
            })
    }

    'prune'
    {
        $success = Invoke-PruneWorktrees
        exit $(if ($success)
            { 0 
            } else
            { 1 
            })
    }

    'clean'
    {
        $success = Invoke-CleanWorktrees -Force:$Force
        exit $(if ($success)
            { 0 
            } else
            { 1 
            })
    }

    default
    {
        Write-Error "Unknown command: $Command"
        Show-Help
        exit 1
    }
}
