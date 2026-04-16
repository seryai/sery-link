#!/bin/bash
# Run all tests for Sery Link

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "🧪 Running Sery Link Test Suite"
echo "================================"
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Function to print test result
print_result() {
    local test_name=$1
    local status=$2

    if [ "$status" = "PASS" ]; then
        echo -e "${GREEN}✅ $test_name${NC}"
        ((PASSED_TESTS++))
    elif [ "$status" = "FAIL" ]; then
        echo -e "${RED}❌ $test_name${NC}"
        ((FAILED_TESTS++))
    else
        echo -e "${YELLOW}⏭️  $test_name (skipped)${NC}"
    fi
    ((TOTAL_TESTS++))
}

echo "📦 1. TypeScript Type Check"
echo "----------------------------"
cd "$PROJECT_ROOT"
if npm run build > /dev/null 2>&1; then
    print_result "TypeScript compilation" "PASS"
else
    print_result "TypeScript compilation" "FAIL"
    echo "Run 'npm run build' to see errors"
fi
echo ""

echo "🦀 2. Rust Tests"
echo "----------------------------"
cd "$PROJECT_ROOT/src-tauri"

# Run Rust tests and capture output
if cargo test --lib --quiet 2>&1 | grep -q "test result: ok"; then
    TEST_OUTPUT=$(cargo test --lib --quiet 2>&1)
    RUST_PASSED=$(echo "$TEST_OUTPUT" | grep -oE "[0-9]+ passed" | grep -oE "[0-9]+")
    RUST_FAILED=$(echo "$TEST_OUTPUT" | grep -oE "[0-9]+ failed" | grep -oE "[0-9]+" || echo "0")

    print_result "Rust unit tests ($RUST_PASSED passed)" "PASS"

    if [ "$RUST_FAILED" != "0" ]; then
        print_result "Rust tests failed ($RUST_FAILED failures)" "FAIL"
    fi
else
    print_result "Rust unit tests" "FAIL"
    echo "Run 'cargo test --lib' to see errors"
fi
echo ""

echo "🔍 3. Rust Linting"
echo "----------------------------"
if cargo clippy --quiet -- -D warnings > /dev/null 2>&1; then
    print_result "Clippy lints" "PASS"
else
    print_result "Clippy lints" "FAIL"
    echo "Run 'cargo clippy' to see warnings"
fi
echo ""

echo "📝 4. Rust Formatting"
echo "----------------------------"
if cargo fmt -- --check > /dev/null 2>&1; then
    print_result "Rust formatting" "PASS"
else
    print_result "Rust formatting" "FAIL"
    echo "Run 'cargo fmt' to fix formatting"
fi
echo ""

echo "🔒 5. Security Audit (Rust)"
echo "----------------------------"
if cargo audit > /dev/null 2>&1; then
    print_result "Security vulnerabilities" "PASS"
else
    # cargo audit might not be installed
    if ! command -v cargo-audit &> /dev/null; then
        print_result "Security audit (not installed)" "SKIP"
        echo "Install: cargo install cargo-audit"
    else
        print_result "Security vulnerabilities" "FAIL"
        echo "Run 'cargo audit' to see details"
    fi
fi
echo ""

# Summary
echo "================================"
echo "📊 Test Summary"
echo "================================"
echo "Total tests: $TOTAL_TESTS"
echo -e "${GREEN}Passed: $PASSED_TESTS${NC}"
echo -e "${RED}Failed: $FAILED_TESTS${NC}"
echo ""

if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "${GREEN}🎉 All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
fi
