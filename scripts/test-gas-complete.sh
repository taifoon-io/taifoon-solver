#!/bin/bash
# Comprehensive Gas Price Integration Test
# Tests Warmbed API + Solver integration for accurate gas cost calculations

set -e

WARMBED_API="http://46.4.96.124:30081"
SOLVER_API="http://localhost:8082"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 GAS PRICE INTEGRATION - COMPLETE TEST SUITE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# Test via SSH since direct access times out
echo "📡 Testing Warmbed API (via SSH tunnel):"
echo "─────────────────────────────────────────────────"

# Test key chains that Taifoon supports
declare -a TEST_CHAINS=(
    "1:Ethereum"
    "10:Optimism"
    "8453:Base"
    "42161:Arbitrum"
    "137:Polygon"
    "56:BSC"
    "43114:Avalanche"
    "250:Fantom"
    "324:zkSync"
    "59144:Linea"
)

SUCCESS_COUNT=0
FAIL_COUNT=0

echo "Chain  | Name         | Status  | Gas (gwei) | Block"
echo "───────┼──────────────┼─────────┼────────────┼───────────"

for chain_spec in "${TEST_CHAINS[@]}"; do
    IFS=':' read -r chain_id chain_name <<< "$chain_spec"

    # Test via SSH since direct access may timeout
    RESPONSE=$(ssh root@46.4.96.124 "curl -s -m 2 http://localhost:8081/api/gas/latest/$chain_id" 2>&1)

    if echo "$RESPONSE" | jq -e '.gas_price_gwei' > /dev/null 2>&1; then
        gas_price=$(echo "$RESPONSE" | jq -r '.gas_price_gwei')
        block_num=$(echo "$RESPONSE" | jq -r '.block_number')
        printf "%-6s │ %-12s │ ✅ OK    │ %-10s │ %s\n" "$chain_id" "$chain_name" "$gas_price" "$block_num"
        ((SUCCESS_COUNT++))
    else
        printf "%-6s │ %-12s │ ❌ FAIL  │ -          │ -\n" "$chain_id" "$chain_name"
        ((FAIL_COUNT++))
    fi
done

echo
echo "Summary: $SUCCESS_COUNT/$((SUCCESS_COUNT + FAIL_COUNT)) chains returning gas data"

# Test solver integration
echo
echo "🔧 Testing Solver Integration:"
echo "─────────────────────────────────────────────────"

echo -n "1. Solver API health... "
if curl -s -f -m 2 "$SOLVER_API/api/solver/stats" > /dev/null 2>&1; then
    echo "✅ OK"
else
    echo "❌ FAILED (not running?)"
    exit 1
fi

echo -n "2. Solver stats... "
STATS=$(curl -s -m 2 "$SOLVER_API/api/solver/stats" 2>&1)
if [ $? -eq 0 ]; then
    TOTAL=$(echo "$STATS" | jq -r '.total_intents // 0')
    PROFIT=$(echo "$STATS" | jq -r '.profitable_intents // 0')
    echo "✅ OK (Total: $TOTAL, Profitable: $PROFIT)"
else
    echo "❌ FAILED"
fi

echo -n "3. Recent intents with gas costs... "
INTENTS=$(curl -s -m 2 "$SOLVER_API/api/solver/intents" 2>&1)
if echo "$INTENTS" | jq -e '.intents[0]' > /dev/null 2>&1; then
    HAS_GAS=$(echo "$INTENTS" | jq '.intents[] | select(.gas_cost_usd != null) | .gas_cost_usd' | head -1)
    if [ ! -z "$HAS_GAS" ] && [ "$HAS_GAS" != "null" ]; then
        COUNT=$(echo "$INTENTS" | jq '[.intents[] | select(.gas_cost_usd != null)] | length')
        echo "✅ OK ($COUNT intents with gas costs)"

        echo
        echo "   Sample intents with gas data:"
        echo "$INTENTS" | jq -r '.intents[] | select(.gas_cost_usd != null) | "   • Chain \(.src_chain) → \(.dst_chain): Gas $\(.gas_cost_usd | tonumber | . * 100 | round / 100), Profit $\(.profit_usd // 0 | tonumber | . * 100 | round / 100)"' | head -3
    else
        echo "⚠️  No gas cost data in recent intents"
    fi
else
    echo "⚠️  No recent intents"
fi

# Data quality checks
echo
echo "📊 Data Quality Validation:"
echo "─────────────────────────────────────────────────"

# Get Ethereum data for quality check
ETH_DATA=$(ssh root@46.4.96.124 "curl -s http://localhost:8081/api/gas/latest/1" 2>&1)

echo -n "1. Required fields present... "
if echo "$ETH_DATA" | jq -e '.chain_id and .block_number and .gas_price_gwei and .timestamp' > /dev/null 2>&1; then
    echo "✅"
else
    echo "❌"
fi

echo -n "2. Gas price reasonable... "
GAS_PRICE=$(echo "$ETH_DATA" | jq -r '.gas_price_gwei // 0')
if [ $(echo "$GAS_PRICE > 0 && $GAS_PRICE < 1000" | bc -l) -eq 1 ]; then
    echo "✅ ($GAS_PRICE gwei)"
else
    echo "❌ ($GAS_PRICE gwei)"
fi

echo -n "3. Data freshness (< 1 minute old)... "
TIMESTAMP=$(echo "$ETH_DATA" | jq -r '.timestamp')
NOW=$(date +%s)
AGE=$((NOW - TIMESTAMP))
if [ $AGE -lt 60 ]; then
    echo "✅ (${AGE}s old)"
else
    echo "⚠️  (${AGE}s old)"
fi

# Performance test
echo
echo "⚡ Performance Test:"
echo "─────────────────────────────────────────────────"

echo -n "Testing 5 sequential requests to chain 1... "
START=$(perl -MTime::HiRes -e 'print Time::HiRes::time()')
for i in {1..5}; do
    ssh root@46.4.96.124 "curl -s -m 2 http://localhost:8081/api/gas/latest/1" > /dev/null 2>&1
done
END=$(perl -MTime::HiRes -e 'print Time::HiRes::time()')
DURATION=$(echo "($END - $START) * 1000" | bc)
AVG=$(echo "$DURATION / 5" | bc)
echo "${AVG}ms avg"

if [ $(echo "$AVG < 100" | bc -l) -eq 1 ]; then
    echo "✅ Performance: Excellent (< 100ms)"
elif [ $(echo "$AVG < 500" | bc -l) -eq 1 ]; then
    echo "✅ Performance: Good (< 500ms)"
else
    echo "⚠️  Performance: Slow (> 500ms)"
fi

# Recommendations
echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📝 RECOMMENDATIONS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ "$SUCCESS_COUNT" -gt 8 ]; then
    echo "✅ Warmbed API has excellent coverage ($SUCCESS_COUNT chains)"
    echo "   Solver is ready to use real-time gas prices"
    echo
    echo "   Current configuration (via environment):"
    echo "   WARMBED_API_URL=http://46.4.96.124:30081"
    echo "   GAS_CACHE_TTL=30 seconds"
else
    echo "⚠️  Limited gas data coverage ($SUCCESS_COUNT chains)"
    echo
    echo "   Action items:"
    echo "   1. Check spinner logs: kubectl logs -n spinner spinner-0"
    echo "   2. Verify RPC connectivity for failed chains"
    echo "   3. Check gas metrics CF population"
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🏁 Test suite complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
