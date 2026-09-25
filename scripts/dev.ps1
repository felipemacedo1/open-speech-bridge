<#
.SYNOPSIS
    OpenSpeechBridge Development Helper Script

.DESCRIPTION
    Provides commands for Docker-based development workflow.

.PARAMETER Command
    The command to execute: build, shell, test, lint, clean, logs, stop

.EXAMPLE
    .\scripts\dev.ps1 build    # Build the development container
    .\scripts\dev.ps1 shell    # Enter the development container
    .\scripts\dev.ps1 test     # Run tests in container
    .\scripts\dev.ps1 lint     # Run linting in container
#>

param(
    [Parameter(Position=0)]
    [ValidateSet("build", "shell", "test", "lint", "fmt", "clean", "logs", "stop", "status", "help")]
    [string]$Command = "help"
)

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $ProjectRoot

function Show-Help {
    Write-Host @"
OpenSpeechBridge Development Commands
=====================================

Usage: .\scripts\dev.ps1 <command>

Commands:
    build   - Build the Docker development container
    shell   - Start container and open interactive shell
    test    - Run all tests in container
    lint    - Run clippy linting
    fmt     - Format code with rustfmt
    clean   - Remove containers and volumes
    logs    - Show container logs
    stop    - Stop the development container
    status  - Show container status
    help    - Show this help message

First time setup:
    1. Ensure Docker Desktop is running
    2. Run: .\scripts\dev.ps1 build
    3. Run: .\scripts\dev.ps1 shell
    4. Inside container: cargo build

Proxy Configuration:
    The .env file contains proxy settings. Edit if needed.
"@
}

function Test-DockerRunning {
    try {
        docker info 2>&1 | Out-Null
        return $LASTEXITCODE -eq 0
    } catch {
        return $false
    }
}

function Invoke-Build {
    Write-Host "Building development container..." -ForegroundColor Cyan
    docker-compose build dev
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Build completed successfully!" -ForegroundColor Green
    } else {
        Write-Host "Build failed!" -ForegroundColor Red
        exit 1
    }
}

function Invoke-Shell {
    Write-Host "Starting development container..." -ForegroundColor Cyan
    
    # Start container if not running
    $running = docker-compose ps -q dev 2>$null
    if (-not $running) {
        docker-compose up -d dev
    }
    
    # Enter shell
    docker-compose exec dev bash
}

function Invoke-Test {
    Write-Host "Running tests..." -ForegroundColor Cyan
    docker-compose exec dev cargo test --all-targets
}

function Invoke-Lint {
    Write-Host "Running clippy..." -ForegroundColor Cyan
    docker-compose exec dev cargo clippy --all-targets -- -D warnings
}

function Invoke-Format {
    Write-Host "Formatting code..." -ForegroundColor Cyan
    docker-compose exec dev cargo fmt
}

function Invoke-Clean {
    Write-Host "Cleaning up containers and volumes..." -ForegroundColor Yellow
    docker-compose down -v
    Write-Host "Cleanup complete!" -ForegroundColor Green
}

function Show-Logs {
    docker-compose logs -f dev
}

function Invoke-Stop {
    Write-Host "Stopping containers..." -ForegroundColor Yellow
    docker-compose down
    Write-Host "Containers stopped." -ForegroundColor Green
}

function Show-Status {
    Write-Host "Container Status:" -ForegroundColor Cyan
    docker-compose ps
}

# Check Docker is running
if ($Command -ne "help" -and -not (Test-DockerRunning)) {
    Write-Host "Error: Docker is not running!" -ForegroundColor Red
    Write-Host "Please start Docker Desktop and try again." -ForegroundColor Yellow
    exit 1
}

# Execute command
switch ($Command) {
    "build"  { Invoke-Build }
    "shell"  { Invoke-Shell }
    "test"   { Invoke-Test }
    "lint"   { Invoke-Lint }
    "fmt"    { Invoke-Format }
    "clean"  { Invoke-Clean }
    "logs"   { Show-Logs }
    "stop"   { Invoke-Stop }
    "status" { Show-Status }
    "help"   { Show-Help }
    default  { Show-Help }
}
