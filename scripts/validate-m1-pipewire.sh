#!/usr/bin/env bash
# =============================================================================
# validate-m1-pipewire.sh - M1 PipeWire Integration Validation Script
# =============================================================================
# Run this on a physical Ubuntu system with PipeWire daemon to validate
# the loopback functionality before merging PR #14.
#
# Requirements:
#   - Ubuntu 22.04+ with PipeWire as the audio server
#   - Rust toolchain (cargo)
#   - pipewire, pw-dump, pw-link, pw-cli commands available
#
# Usage:
#   ./scripts/validate-m1-pipewire.sh [--duration SECONDS]
#
# Output:
#   - Creates artifacts in ./validation-artifacts/
#   - Prints PASS/FAIL summary at the end
# =============================================================================

set -euo pipefail

# Configuration
DURATION="${1:-10}"
ARTIFACT_DIR="./validation-artifacts"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
ARTIFACT_SUBDIR="${ARTIFACT_DIR}/${TIMESTAMP}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Track test results
TESTS_PASSED=0
TESTS_FAILED=0

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_test() {
    local result=$1
    local name=$2
    if [ "$result" -eq 0 ]; then
        echo -e "${GREEN}[PASS]${NC} $name"
        ((TESTS_PASSED++))
    else
        echo -e "${RED}[FAIL]${NC} $name"
        ((TESTS_FAILED++))
    fi
}

# =============================================================================
# Environment Checks
# =============================================================================

check_environment() {
    log_info "Checking environment..."
    
    # Check OS
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        log_info "OS: $PRETTY_NAME"
    else
        log_warn "Could not determine OS version"
    fi
    
    # Check PipeWire daemon
    if pgrep -x pipewire > /dev/null 2>&1; then
        log_test 0 "PipeWire daemon running"
    else
        log_test 1 "PipeWire daemon running"
        log_error "PipeWire daemon is not running. Start it with: systemctl --user start pipewire"
        exit 1
    fi
    
    # Check PipeWire tools
    local tools=("pw-dump" "pw-link" "pw-cli" "pactl")
    for tool in "${tools[@]}"; do
        if command -v "$tool" > /dev/null 2>&1; then
            log_test 0 "Tool available: $tool"
        else
            log_test 1 "Tool available: $tool"
            log_warn "$tool not found - some diagnostics may be limited"
        fi
    done
    
    # Check Rust/Cargo
    if command -v cargo > /dev/null 2>&1; then
        local cargo_version=$(cargo --version)
        log_test 0 "Cargo available: $cargo_version"
    else
        log_test 1 "Cargo available"
        log_error "Cargo not found. Install Rust toolchain first."
        exit 1
    fi
    
    # Check we're in the right directory
    if [ -f "Cargo.toml" ] && grep -q "osb-cli" Cargo.toml; then
        log_test 0 "In OpenSpeechBridge workspace root"
    else
        log_test 1 "In OpenSpeechBridge workspace root"
        log_error "Run this script from the workspace root (where Cargo.toml is)"
        exit 1
    fi
}

# =============================================================================
# Pre-flight Artifacts
# =============================================================================

collect_preflight_artifacts() {
    log_info "Collecting pre-flight artifacts..."
    
    mkdir -p "$ARTIFACT_SUBDIR"
    
    # PipeWire state before test
    if command -v pw-dump > /dev/null 2>&1; then
        pw-dump > "${ARTIFACT_SUBDIR}/pw-dump-before.json" 2>&1 || true
        log_info "Saved: pw-dump-before.json"
    fi
    
    if command -v pw-link > /dev/null 2>&1; then
        pw-link -o > "${ARTIFACT_SUBDIR}/pw-link-before.txt" 2>&1 || true
        log_info "Saved: pw-link-before.txt"
    fi
    
    # System audio info
    if command -v pactl > /dev/null 2>&1; then
        pactl info > "${ARTIFACT_SUBDIR}/pactl-info.txt" 2>&1 || true
        pactl list sources > "${ARTIFACT_SUBDIR}/pactl-sources.txt" 2>&1 || true
        pactl list sinks > "${ARTIFACT_SUBDIR}/pactl-sinks.txt" 2>&1 || true
        log_info "Saved: pactl info/sources/sinks"
    fi
}

# =============================================================================
# Build Project
# =============================================================================

build_project() {
    log_info "Building project..."
    
    if cargo build --release 2>&1 | tee "${ARTIFACT_SUBDIR}/build.log"; then
        log_test 0 "Cargo build succeeded"
    else
        log_test 1 "Cargo build succeeded"
        log_error "Build failed. Check ${ARTIFACT_SUBDIR}/build.log"
        exit 1
    fi
}

# =============================================================================
# Run Tests
# =============================================================================

run_tests() {
    log_info "Running cargo tests..."
    
    if cargo test 2>&1 | tee "${ARTIFACT_SUBDIR}/test.log"; then
        log_test 0 "Cargo tests passed"
    else
        log_test 1 "Cargo tests passed"
        log_warn "Some tests failed. Check ${ARTIFACT_SUBDIR}/test.log"
    fi
}

# =============================================================================
# Doctor Check
# =============================================================================

run_doctor() {
    log_info "Running openspeechbridge doctor..."
    
    if cargo run --release --bin openspeechbridge -- doctor 2>&1 | tee "${ARTIFACT_SUBDIR}/doctor.log"; then
        log_test 0 "Doctor check passed"
    else
        log_test 1 "Doctor check passed"
        log_warn "Doctor reported issues. Check ${ARTIFACT_SUBDIR}/doctor.log"
    fi
}

# =============================================================================
# Loopback Test
# =============================================================================

run_loopback() {
    log_info "Running loopback test for ${DURATION} seconds..."
    
    # Start loopback in background
    timeout "${DURATION}s" cargo run --release --bin openspeechbridge -- loopback \
        2>&1 | tee "${ARTIFACT_SUBDIR}/loopback.log" &
    local loopback_pid=$!
    
    # Wait a moment for it to start
    sleep 2
    
    # Collect PipeWire state during test
    if command -v pw-dump > /dev/null 2>&1; then
        pw-dump > "${ARTIFACT_SUBDIR}/pw-dump-during.json" 2>&1 || true
        log_info "Saved: pw-dump-during.json"
    fi
    
    if command -v pw-link > /dev/null 2>&1; then
        pw-link -o > "${ARTIFACT_SUBDIR}/pw-link-during.txt" 2>&1 || true
        log_info "Saved: pw-link-during.txt"
    fi
    
    # Wait for loopback to complete
    if wait $loopback_pid 2>/dev/null; then
        log_test 0 "Loopback completed without crash"
    else
        local exit_code=$?
        if [ $exit_code -eq 124 ]; then
            # timeout exit code - this is expected
            log_test 0 "Loopback completed (timeout as expected)"
        else
            log_test 1 "Loopback completed without crash (exit: $exit_code)"
        fi
    fi
    
    # Analyze loopback log
    if [ -f "${ARTIFACT_SUBDIR}/loopback.log" ]; then
        # Check for panic/crash
        if grep -qi "panic\|SIGSEGV\|SIGABRT\|fatal" "${ARTIFACT_SUBDIR}/loopback.log"; then
            log_test 1 "No panic/crash in loopback"
        else
            log_test 0 "No panic/crash in loopback"
        fi
        
        # Check for buffer overflow messages
        local overflow_count=$(grep -c "overflow\|dropped" "${ARTIFACT_SUBDIR}/loopback.log" 2>/dev/null || echo "0")
        if [ "$overflow_count" -gt 10 ]; then
            log_test 1 "Buffer overflow count acceptable (found: $overflow_count)"
        else
            log_test 0 "Buffer overflow count acceptable (found: $overflow_count)"
        fi
        
        # Check for underrun messages
        local underrun_count=$(grep -c "underrun" "${ARTIFACT_SUBDIR}/loopback.log" 2>/dev/null || echo "0")
        log_info "Underrun count: $underrun_count (informational)"
    fi
}

# =============================================================================
# Post-flight Artifacts
# =============================================================================

collect_postflight_artifacts() {
    log_info "Collecting post-flight artifacts..."
    
    if command -v pw-dump > /dev/null 2>&1; then
        pw-dump > "${ARTIFACT_SUBDIR}/pw-dump-after.json" 2>&1 || true
        log_info "Saved: pw-dump-after.json"
    fi
    
    if command -v pw-link > /dev/null 2>&1; then
        pw-link -o > "${ARTIFACT_SUBDIR}/pw-link-after.txt" 2>&1 || true
        log_info "Saved: pw-link-after.txt"
    fi
    
    # Save system info
    {
        echo "=== Validation Run Info ==="
        echo "Date: $(date)"
        echo "Duration: ${DURATION}s"
        echo "Hostname: $(hostname)"
        echo "Kernel: $(uname -r)"
        echo "User: $(whoami)"
        echo ""
        echo "=== Git Info ==="
        git log -1 --oneline 2>/dev/null || echo "Not a git repo"
        git status --short 2>/dev/null || true
        echo ""
        echo "=== Rust Info ==="
        rustc --version 2>/dev/null || echo "rustc not found"
        cargo --version 2>/dev/null || echo "cargo not found"
    } > "${ARTIFACT_SUBDIR}/system-info.txt"
    log_info "Saved: system-info.txt"
}

# =============================================================================
# Summary
# =============================================================================

print_summary() {
    echo ""
    echo "=============================================="
    echo "          M1 VALIDATION SUMMARY"
    echo "=============================================="
    echo ""
    echo -e "Tests Passed: ${GREEN}${TESTS_PASSED}${NC}"
    echo -e "Tests Failed: ${RED}${TESTS_FAILED}${NC}"
    echo ""
    echo "Artifacts saved to: ${ARTIFACT_SUBDIR}"
    echo ""
    
    if [ "$TESTS_FAILED" -eq 0 ]; then
        echo -e "${GREEN}╔═══════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}║           ALL TESTS PASSED!               ║${NC}"
        echo -e "${GREEN}║    M1 ready for merge after PR review     ║${NC}"
        echo -e "${GREEN}╚═══════════════════════════════════════════╝${NC}"
        exit 0
    else
        echo -e "${RED}╔═══════════════════════════════════════════╗${NC}"
        echo -e "${RED}║         SOME TESTS FAILED                 ║${NC}"
        echo -e "${RED}║    Review artifacts before proceeding     ║${NC}"
        echo -e "${RED}╚═══════════════════════════════════════════╝${NC}"
        exit 1
    fi
}

# =============================================================================
# Main
# =============================================================================

main() {
    echo "=============================================="
    echo "  OpenSpeechBridge M1 PipeWire Validation"
    echo "=============================================="
    echo ""
    
    check_environment
    echo ""
    
    collect_preflight_artifacts
    echo ""
    
    build_project
    echo ""
    
    run_tests
    echo ""
    
    run_doctor
    echo ""
    
    run_loopback
    echo ""
    
    collect_postflight_artifacts
    echo ""
    
    print_summary
}

# Handle --duration flag
if [ "${1:-}" = "--duration" ] && [ -n "${2:-}" ]; then
    DURATION="$2"
fi

main
