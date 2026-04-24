#!/bin/bash
# Comprehensive Gas Price API Integration Test Suite
# Tests Warmbed/Spinner gas oracle across all supported chains

set -e

WARMBED_API="http://46.4.96.124:30081"
LOCAL_SOLVER_API="http://localhost:8082"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 GAS PRICE API INTEGRATION TEST SUITE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# Common chains for cross-chain bridge protocols
declare -A CHAINS=(
    [1]="Ethereum"
    [10]="Optimism"
    [8453]="Base"
    [42161]="Arbitrum"
    [56]="BSC"
    [137]="Polygon"
    [43114]="Avalanche"
    [250]="Fantom"
    [100]="Gnosis"
    [324]="zkSync Era"
    [59144]="Linea"
    [534352]="Scroll"
    [81457]="Blast"
    [34443]="Mode"
    [7777777]="Zora"
    [252]="Fraxtal"
    [1135]="Lisk"
    [957]="Lyra"
)

echo "📊 Testing Warmbed API Endpoints"
echo "─────────────────────────────────────────────────"

# Test 1: Health check
echo -n "1. Warmbed API health check... "
if curl -s -f "$WARMBED_API/health" > /dev/null 2>&1; then
    echo "✅ OK"
else
    echo "❌ FAILED"
fi

# Test 2: All chains gas endpoint
echo -n "2. Fetching all chains gas data... "
ALL_GAS=$(curl -s "$WARMBED_API/api/gas/latest" 2>&1)
if [ $? -eq 0 ] && [ ! -z "$ALL_GAS" ]; then
    COUNT=$(echo "$ALL_GAS" | jq '. | length' 2>/dev/null || echo "0")
    echo "✅ OK ($COUNT chains)"
else
    echo "❌ FAILED"
    ALL_GAS="[]"
fi

# Test 3: Per-chain gas prices
echo
echo "3. Testing per-chain gas price endpoints:"
echo "   Chain ID | Name            | Status | Gas Price (gwei)"
echo "   ─────────┼─────────────────┼────────┼────────────────"

SUCCESS_COUNT=0
FAIL_COUNT=0

for chain_id in "${!CHAINS[@]}"; do
    chain_name="${CHAINS[$chain_id]}"

    # Try to fetch gas price
    RESPONSE=$(curl -s -m 2 "$WARMBED_API/api/gas/latest/$chain_id" 2>&1)

    if [ $? -eq 0 ] && echo "$RESPONSE" | jq -e '.gas_price_gwei' > /dev/null 2>&1; then
        gas_price=$(echo "$RESPONSE" | jq -r '.gas_price_gwei')
        printf "   %-9s│ %-15s │ ✅ OK   │ %s\n" "$chain_id" "$chain_name" "$gas_price"
        ((SUCCESS_COUNT++))
    else
        printf "   %-9s│ %-15s │ ❌ FAIL │ -\n" "$chain_id" "$chain_name"
        ((FAIL_COUNT++))
    fi
done

echo
echo "Summary: $SUCCESS_COUNT/$((SUCCESS_COUNT + FAIL_COUNT)) chains returning gas data"

# Test 4: Data quality checks
echo
echo "4. Data quality validation:"
if [ "$SUCCESS_COUNT" -gt 0 ]; then
    # Pick first successful chain (Ethereum)
    ETH_DATA=$(curl -s "$WARMBED_API/api/gas/latest/1" 2>&1)

    echo -n "   - Has chain_id field... "
    if echo "$ETH_DATA" | jq -e '.chain_id' > /dev/null 2>&1; then
        echo "✅"
    else
        echo "❌"
    fi

    echo -n "   - Has block_number field... "
    if echo "$ETH_DATA" | jq -e '.block_number' > /dev/null 2>&1; then
        echo "✅"
    else
        echo "❌"
    fi

    echo -n "   - Has gas_price_gwei field... "
    if echo "$ETH_DATA" | jq -e '.gas_price_gwei' > /dev/null 2>&1; then
        echo "✅"
    else
        echo "❌"
    fi

    echo -n "   - Has timestamp field... "
    if echo "$ETH_DATA" | jq -e '.timestamp' > /dev/null 2>&1; then
        echo "✅"
    else
        echo "❌"
    fi

    echo -n "   - Gas price is reasonable (< 1000 gwei)... "
    GAS_PRICE=$(echo "$ETH_DATA" | jq -r '.gas_price_gwei // 0')
    if [ "$(echo "$GAS_PRICE < 1000" | bc)" -eq 1 ]; then
        echo "✅ ($GAS_PRICE gwei)"
    else
        echo "❌ ($GAS_PRICE gwei)"
    fi
else
    echo "   ⚠️  No successful responses to validate"
fi

# Test 5: Solver integration test
echo
echo "5. Solver gas price integration:"
echo -n "   - Solver API health... "
if curl -s -f "$LOCAL_SOLVER_API/api/solver/stats" > /dev/null 2>&1; then
    echo "✅ OK"

    # Check recent intents to see if they're using real gas prices
    INTENTS=$(curl -s "$LOCAL_SOLVER_API/api/solver/intents" 2>&1)
    if echo "$INTENTS" | jq -e '.intents[0]' > /dev/null 2>&1; then
        echo "   - Recent intents found... ✅"
        INTENT_COUNT=$(echo "$INTENTS" | jq '.intents | length')
        echo "   - Total intents processed: $INTENT_COUNT"
    fi
else
    echo "❌ FAILED (solver not running?)"
fi

# Test 6: Performance test
echo
echo "6. Performance test (10 sequential requests to chain 1):"
START=$(date +%s%N)
for i in {1..10}; do
    curl -s "$WARMBED_API/api/gas/latest/1" > /dev/null 2>&1
done
END=$(date +%s%N)
DURATION=$(( (END - START) / 1000000 ))
AVG=$(( DURATION / 10 ))
echo "   Total: ${DURATION}ms, Average: ${AVG}ms per request"
if [ $AVG -lt 100 ]; then
    echo "   ✅ Performance: Excellent (< 100ms avg)"
elif [ $AVG -lt 500 ]; then
    echo "   ✅ Performance: Good (< 500ms avg)"
else
    echo "   ⚠️  Performance: Slow (> 500ms avg)"
fi

# Test 7: Recommend configuration
echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📝 RECOMMENDATIONS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ "$SUCCESS_COUNT" -gt 10 ]; then
    echo "✅ Warmbed API is working well ($SUCCESS_COUNT chains)"
    echo "   Solver can use real-time gas prices for most chains"
    echo
    echo "   Recommended solver configuration:"
    echo "   WARMBED_API_URL=http://46.4.96.124:30081"
    echo "   GAS_CACHE_TTL=30  # seconds"
else
    echo "⚠️  Warmbed API has limited data ($SUCCESS_COUNT chains)"
    echo "   Solver should use fallback estimates for most chains"
    echo
    echo "   Action items:"
    echo "   1. Check warmbed-collector logs"
    echo "   2. Verify RPC connectivity"
    echo "   3. Ensure gas metrics CF is being populated"
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🏁 Test suite complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
