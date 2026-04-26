#!/bin/bash
# Automated Fixture Validator Script
# Validates all protocol fixtures against Razor API with real gas prices
# Catches: >$1 profits, None values, 0 gas values, unrealistic costs

set -e

RAZOR_API="http://localhost:9081/api/solver/razor"
FIXTURES_DIR="fixtures"

echo "========================================================================"
echo "TAIFOON FIXTURE VALIDATOR"
echo "========================================================================"
echo ""

# Fetch gas prices from Razor API
echo "📡 Fetching gas prices from Razor API..."
GAS_DATA=$(curl -s "$RAZOR_API")

if [ $? -ne 0 ]; then
    echo "❌ Failed to fetch gas prices from $RAZOR_API"
    echo "   Make sure taifoon-solver is running on localhost:9081"
    exit 1
fi

echo "✅ Fetched gas prices"
echo ""

# Extract gas prices per chain
echo "Gas Prices:"
echo "$GAS_DATA" | jq -r '.presets[] | "  Chain \(.chain_id): \(.gas_cost_gwei // .gas_price_gwei // "N/A") gwei"'
echo ""

# Protocol files
PROTOCOLS=(
    "across_v3"
    "allbridge"
    "hyperlane"
    "layerzero_v2"
    "lifi_v2"
    "orbiter_finance"
    "squid_router"
    "stargate_v2"
    "t3rn_lwc"
)

TOTAL=0
PASSED=0
FAILED=0
WARNINGS=0

FAILURES_FILE=$(mktemp)
WARNINGS_FILE=$(mktemp)

trap "rm -f $FAILURES_FILE $WARNINGS_FILE" EXIT

for PROTOCOL in "${PROTOCOLS[@]}"; do
    FIXTURE_FILE="$FIXTURES_DIR/${PROTOCOL}.json"

    if [ ! -f "$FIXTURE_FILE" ]; then
        echo "⚠️  Skipping $PROTOCOL (file not found)"
        continue
    fi

    echo "🔍 Testing $PROTOCOL fixtures..."

    # Count intents in this fixture
    INTENT_COUNT=$(jq 'length' "$FIXTURE_FILE")

    # Validate each intent
    for i in $(seq 0 $((INTENT_COUNT - 1))); do
        INTENT=$(jq ".[$i]" "$FIXTURE_FILE")

        INTENT_ID=$(echo "$INTENT" | jq -r '.id // "unknown"')
        SRC_CHAIN=$(echo "$INTENT" | jq -r '.src_chain // 0')
        DST_CHAIN=$(echo "$INTENT" | jq -r '.dst_chain // 0')
        RAW_PROFIT=$(echo "$INTENT" | jq -r '.profit_usd // "null"')

        ISSUES=()
        STATUS="PASS"

        # Get gas prices for src and dst chains
        SRC_GAS=$(echo "$GAS_DATA" | jq -r ".presets[] | select(.chain_id == $SRC_CHAIN) | .gas_cost_gwei // .gas_price_gwei // \"null\"")
        DST_GAS=$(echo "$GAS_DATA" | jq -r ".presets[] | select(.chain_id == $DST_CHAIN) | .gas_cost_gwei // .gas_price_gwei // \"null\"")

        SRC_GAS_USD=$(echo "$GAS_DATA" | jq -r ".presets[] | select(.chain_id == $SRC_CHAIN) | .gas_cost_usd // \"null\"")
        DST_GAS_USD=$(echo "$GAS_DATA" | jq -r ".presets[] | select(.chain_id == $DST_CHAIN) | .gas_cost_usd // \"null\"")

        # Check for missing gas prices
        if [ "$SRC_GAS" == "null" ] || [ -z "$SRC_GAS" ]; then
            ISSUES+=("Missing gas price for src chain $SRC_CHAIN")
            STATUS="WARNING"
        fi

        if [ "$DST_GAS" == "null" ] || [ -z "$DST_GAS" ]; then
            ISSUES+=("Missing gas price for dst chain $DST_CHAIN")
            STATUS="WARNING"
        fi

        # Check for zero gas prices
        if [ "$SRC_GAS" == "0" ] || [ "$SRC_GAS" == "0.0" ]; then
            ISSUES+=("Zero gas price for src chain $SRC_CHAIN")
            STATUS="FAIL"
        fi

        if [ "$DST_GAS" == "0" ] || [ "$DST_GAS" == "0.0" ]; then
            ISSUES+=("Zero gas price for dst chain $DST_CHAIN")
            STATUS="FAIL"
        fi

        # Check for None gas_cost_usd
        if [ "$SRC_GAS_USD" == "null" ] || [ -z "$SRC_GAS_USD" ]; then
            ISSUES+=("Missing gas_cost_usd for src chain $SRC_CHAIN")
            STATUS="WARNING"
        fi

        if [ "$DST_GAS_USD" == "null" ] || [ -z "$DST_GAS_USD" ]; then
            ISSUES+=("Missing gas_cost_usd for dst chain $DST_CHAIN")
            STATUS="WARNING"
        fi

        # Calculate total gas cost
        if [ "$SRC_GAS_USD" != "null" ] && [ "$DST_GAS_USD" != "null" ] && [ -n "$SRC_GAS_USD" ] && [ -n "$DST_GAS_USD" ]; then
            TOTAL_GAS_COST=$(echo "$SRC_GAS_USD + $DST_GAS_USD" | bc -l)

            # Check if gas cost > $1
            if (( $(echo "$TOTAL_GAS_COST > 1.0" | bc -l) )); then
                ISSUES+=("Gas cost alone is \$$TOTAL_GAS_COST (likely unprofitable)")
                STATUS="FAIL"
            fi
        else
            TOTAL_GAS_COST="N/A"
        fi

        # Check raw profit
        if [ "$RAW_PROFIT" != "null" ]; then
            if (( $(echo "$RAW_PROFIT > 1.0" | bc -l) )); then
                ISSUES+=("Unrealistic profit: \$$RAW_PROFIT (should be < \$1)")
                STATUS="FAIL"
            fi

            if (( $(echo "$RAW_PROFIT < -2.0" | bc -l) )); then
                ISSUES+=("Unrealistic loss: \$$RAW_PROFIT (should be > -\$2)")
                STATUS="FAIL"
            fi
        fi

        TOTAL=$((TOTAL + 1))

        if [ "$STATUS" == "PASS" ]; then
            PASSED=$((PASSED + 1))
        elif [ "$STATUS" == "FAIL" ]; then
            FAILED=$((FAILED + 1))

            # Log failure
            echo "" >> "$FAILURES_FILE"
            echo "❌ $INTENT_ID" >> "$FAILURES_FILE"
            echo "   Protocol: $PROTOCOL" >> "$FAILURES_FILE"
            echo "   Route: $SRC_CHAIN → $DST_CHAIN" >> "$FAILURES_FILE"
            [ "$RAW_PROFIT" != "null" ] && echo "   Raw Profit: \$$RAW_PROFIT" >> "$FAILURES_FILE"
            echo "   Total Gas Cost: \$$TOTAL_GAS_COST" >> "$FAILURES_FILE"
            echo "   Src Gas: $SRC_GAS gwei" >> "$FAILURES_FILE"
            echo "   Dst Gas: $DST_GAS gwei" >> "$FAILURES_FILE"
            echo "   Issues:" >> "$FAILURES_FILE"
            for issue in "${ISSUES[@]}"; do
                echo "     • $issue" >> "$FAILURES_FILE"
            done
        else
            WARNINGS=$((WARNINGS + 1))

            # Log warning
            echo "" >> "$WARNINGS_FILE"
            echo "⚠️  $INTENT_ID" >> "$WARNINGS_FILE"
            echo "   Protocol: $PROTOCOL" >> "$WARNINGS_FILE"
            echo "   Route: $SRC_CHAIN → $DST_CHAIN" >> "$WARNINGS_FILE"
            [ "$RAW_PROFIT" != "null" ] && echo "   Raw Profit: \$$RAW_PROFIT" >> "$WARNINGS_FILE"
            echo "   Issues:" >> "$WARNINGS_FILE"
            for issue in "${ISSUES[@]}"; do
                echo "     • $issue" >> "$WARNINGS_FILE"
            done
        fi
    done

    echo "   ✓ Validated $INTENT_COUNT intents"
done

# Calculate pass rate
PASS_RATE=$(echo "scale=1; ($PASSED * 100) / $TOTAL" | bc)

echo ""
echo "========================================================================"
echo "TEST SUMMARY"
echo "========================================================================"
echo "Total Intents:  $TOTAL"
echo "✅ PASS:        $PASSED ($PASS_RATE%)"
echo "❌ FAIL:        $FAILED"
echo "⚠️  WARNING:    $WARNINGS"
echo "========================================================================"
echo ""

# Print failures
if [ $FAILED -gt 0 ]; then
    echo "========================================================================"
    echo "FAILURES ($FAILED)"
    echo "========================================================================"
    cat "$FAILURES_FILE"
    echo ""
fi

# Print warnings (truncate to first 10)
if [ $WARNINGS -gt 0 ]; then
    echo "========================================================================"
    echo "WARNINGS ($WARNINGS) - showing first 10"
    echo "========================================================================"
    head -40 "$WARNINGS_FILE"
    echo ""
fi

# Exit with failure if any failures found
if [ $FAILED -gt 0 ]; then
    echo "❌ TEST FAILED: $FAILED failing intents (gas pricing bugs still present)"
    exit 1
else
    echo "========================================================================"
    echo "✅ ALL TESTS PASSED (Pass rate: $PASS_RATE%)"
    echo "========================================================================"
    echo ""
fi
