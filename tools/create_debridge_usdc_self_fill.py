#!/usr/bin/env python3
"""
Create a tiny deBridge DLN self-fill order using USDC as the give/take token.

Creates a Base→Arbitrum USDC order where the solver itself is the allowedTakerDst.
The solver will auto-detect and fill this via its eth_getLogs poller.

Requirements on source chain (Base):
  - USDC balance >= GIVE_AMOUNT_USDC + operating expenses (~$0.29 on a $0.50 order)
  - ETH balance >= 0.001 ETH (fixed protocol fee) + gas (~0.0001 ETH)
  - Must approve USDC to DLN_SOURCE address first (see --approve flag)

Current blocker: need 0.001 ETH on Base (currently have ~0.000142 ETH).
Run with --dry-run to see what would happen.

Usage:
    python3 tools/create_debridge_usdc_self_fill.py --dry-run
    python3 tools/create_debridge_usdc_self_fill.py --approve  # approve first
    python3 tools/create_debridge_usdc_self_fill.py            # broadcast order

Environment:
    SOLVER_PRIVATE_KEY   EVM private key (or loaded from macOS keychain 'mamba-messiah-key')
"""

import subprocess
import os
import sys
import json
import time

# === Config ===
# Create order on Base (we have 0.97 USDC but only 0.000142 ETH)
# Fill order on Arbitrum (we have 0.03 USDC but 0.00066 ETH for gas)
# Need on Base: 0.001 ETH (fixed fee) + gas
SRC_CHAIN_ID    = 8453                      # Base (source)
DST_CHAIN_ID    = 42161                     # Arbitrum (dest where solver fills)
GIVE_AMOUNT_RAW = 400_000                   # 0.40 USDC on Base (6 decimals)
# takeAmount must account for operating expenses. ~$0.29 goes to deBridge cross-chain relay.
# So if give=0.40 USDC, take≈0.10 USDC after protocol covers relay costs.
# Use deBridge API estimation — see actual amount below.
TAKE_AMOUNT_RAW = 100_000                   # 0.10 USDC on Arbitrum (lower-bound estimate)
FIXED_FEE_WEI   = 1_000_000_000_000_000    # 0.001 ETH fixed protocol fee

SRC_RPC         = "https://base-rpc.publicnode.com"
DLN_SOURCE_BASE = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66"
USDC_BASE       = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
USDC_ARB        = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"


def get_solver_address(private_key: str) -> str:
    r = subprocess.run(['cast', 'wallet', 'address', private_key],
                       capture_output=True, text=True)
    return r.stdout.strip()


def get_take_amount_from_api() -> int:
    """Use deBridge API to get accurate take amount after operating expenses."""
    import urllib.request
    url = (f"https://api.dln.trade/v1.0/dln/order/create-tx"
           f"?srcChainId={SRC_CHAIN_ID}"
           f"&srcChainTokenIn={USDC_BASE}"
           f"&srcChainTokenInAmount={GIVE_AMOUNT_RAW}"
           f"&dstChainId={DST_CHAIN_ID}"
           f"&dstChainTokenOut={USDC_ARB}"
           f"&dstChainTokenOutRecipient=0x19b3d79a15b643c6a331772c3331838a5703bc49"
           f"&prependOperatingExpenses=false")
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "taifoon-1.0"})
        with urllib.request.urlopen(req, timeout=8) as r:
            data = json.loads(r.read())
        dst_info = data.get('estimation', {}).get('dstChainTokenOut', {})
        rec_amt = dst_info.get('recommendedAmount', str(TAKE_AMOUNT_RAW))
        print(f"  deBridge API estimated take amount: {rec_amt} raw ({int(rec_amt)/1e6:.4f} USDC)")
        return int(rec_amt)
    except Exception as e:
        print(f"  deBridge API error: {e}, using default {TAKE_AMOUNT_RAW}")
        return TAKE_AMOUNT_RAW


def abi_encode_bytes(data: bytes) -> bytes:
    """ABI-encode a dynamic bytes value: length word + padded data."""
    length = len(data)
    padded_len = (length + 31) // 32 * 32
    return length.to_bytes(32, 'big') + data + b'\x00' * (padded_len - length)


def build_create_salted_order_calldata(solver_addr: str, take_amount_raw: int) -> str:
    """Build createSaltedOrder calldata for USDC→USDC self-fill test."""
    solver_bytes = bytes.fromhex(solver_addr.lstrip('0x'))  # 20 bytes
    usdc_base_bytes = bytes.fromhex(USDC_BASE.lstrip('0x'))  # 20 bytes (give token)
    usdc_arb_bytes = bytes.fromhex(USDC_ARB.lstrip('0x'))   # 20 bytes (take token)

    # Static fields (32 bytes each):
    f_give_token = usdc_base_bytes.rjust(32, b'\x00')                    # USDC on Base
    f_give_amount = GIVE_AMOUNT_RAW.to_bytes(32, 'big')
    f_take_amount = take_amount_raw.to_bytes(32, 'big')
    f_take_chain = DST_CHAIN_ID.to_bytes(32, 'big')
    f_patch_auth = solver_bytes.rjust(32, b'\x00')                       # givePatchAuthoritySrc

    # Dynamic fields:
    dyn_take_token = usdc_arb_bytes                                       # 20 bytes
    dyn_receiver = solver_bytes                                           # solver is receiver
    dyn_order_auth = solver_bytes                                         # orderAuthorityAddressDst
    dyn_allowed_taker = solver_bytes                                      # ONLY solver can fill
    dyn_external_call = b''
    dyn_cancel_beneficiary = b''

    # OrderCreation has 11 head slots (5 static + 6 offsets for dynamic fields)
    # Static: giveTokenAddress(1) + giveAmount(1) + [takeTokenAddress offset](1) +
    #         takeAmount(1) + takeChainId(1) + [receiverDst offset](1) +
    #         givePatchAuthoritySrc(1) + [orderAuthorityAddressDst offset](1) +
    #         [allowedTakerDst offset](1) + [externalCall offset](1) + [allowedCancelBeneficiarySrc offset](1)
    # = 11 head slots = 11 * 32 = 352 bytes offset to first dynamic field

    head_count = 11
    base_offset = head_count * 32

    # Dynamic field sizes (length-prefixed):
    def dyn_size(data): return 32 + ((len(data) + 31) // 32) * 32

    offsets = []
    current = 0
    for data in [dyn_take_token, dyn_receiver, dyn_order_auth, dyn_allowed_taker,
                 dyn_external_call, dyn_cancel_beneficiary]:
        offsets.append((base_offset + current).to_bytes(32, 'big'))
        current += dyn_size(data)

    # Build head section:
    head = (
        f_give_token +
        f_give_amount +
        offsets[0] +            # offset to takeTokenAddress
        f_take_amount +
        f_take_chain +
        offsets[1] +            # offset to receiverDst
        f_patch_auth +
        offsets[2] +            # offset to orderAuthorityAddressDst
        offsets[3] +            # offset to allowedTakerDst
        offsets[4] +            # offset to externalCall
        offsets[5]              # offset to allowedCancelBeneficiarySrc
    )

    # Build tail (dynamic fields):
    tail = b''
    for data in [dyn_take_token, dyn_receiver, dyn_order_auth, dyn_allowed_taker,
                 dyn_external_call, dyn_cancel_beneficiary]:
        tail += abi_encode_bytes(data)

    struct_encoded = head + tail

    # Wrap in createSaltedOrder top-level: (OrderCreation, uint64 salt, bytes affiliateFee, bytes permitEnvelope, bytes metadata)
    # Top head: offset to OrderCreation(1) + salt(1) + offset to affiliateFee(1) +
    #           offset to permitEnvelope(1) + offset to metadata(1) = 5 static head slots
    salt = int(time.time()).to_bytes(32, 'big')  # timestamp as salt
    top_head_count = 5
    top_base = top_head_count * 32

    def outer_dyn_size(data_bytes):
        return 32 + ((len(data_bytes) + 31) // 32) * 32

    struct_off = top_base.to_bytes(32, 'big')
    curr = outer_dyn_size(struct_encoded)
    affiliate_off = (top_base + curr).to_bytes(32, 'big')
    curr += 32 + 32  # empty bytes
    permit_off = (top_base + curr).to_bytes(32, 'big')
    curr += 32 + 32
    meta_off = (top_base + curr).to_bytes(32, 'big')

    top_head = struct_off + salt + affiliate_off + permit_off + meta_off
    top_tail = (
        abi_encode_bytes(struct_encoded) +
        abi_encode_bytes(b'') +   # affiliateFee = empty
        abi_encode_bytes(b'') +   # permitEnvelope = empty
        abi_encode_bytes(b'')     # metadata = empty
    )

    selector = bytes.fromhex('b9303701')  # createSaltedOrder
    calldata = selector + top_head + top_tail
    return '0x' + calldata.hex()


def main():
    dry_run = '--dry-run' in sys.argv
    do_approve = '--approve' in sys.argv

    # Load private key
    pk = os.environ.get('SOLVER_PRIVATE_KEY', '')
    if not pk:
        r = subprocess.run(
            ['security', 'find-generic-password', '-s', 'mamba-messiah-key', '-w'],
            capture_output=True, text=True
        )
        pk = r.stdout.strip()
    if not pk:
        print("ERROR: SOLVER_PRIVATE_KEY not set", file=sys.stderr)
        sys.exit(1)

    solver_addr = get_solver_address(pk)
    print(f"Solver address: {solver_addr}")

    # Check balances
    eth_bal_wei = int(subprocess.run(
        ['cast', 'balance', solver_addr, '--rpc-url', SRC_RPC],
        capture_output=True, text=True
    ).stdout.strip() or '0')
    eth_bal_eth = eth_bal_wei / 1e18

    usdc_bal_raw = int(subprocess.run(
        ['cast', 'call', USDC_BASE, 'balanceOf(address)(uint256)', solver_addr,
         '--rpc-url', SRC_RPC],
        capture_output=True, text=True
    ).stdout.strip().split('\n')[0] or '0')
    usdc_bal_usd = usdc_bal_raw / 1e6

    print(f"\nBase chain balances:")
    print(f"  ETH: {eth_bal_eth:.6f} ETH (need {FIXED_FEE_WEI/1e18:.4f} + gas ≈ 0.0012 ETH total)")
    print(f"  USDC: {usdc_bal_usd:.4f} USDC (need {GIVE_AMOUNT_RAW/1e6:.4f} USDC give amount)")

    if eth_bal_wei < FIXED_FEE_WEI:
        shortfall = (FIXED_FEE_WEI - eth_bal_wei) / 1e18
        print(f"\n⚠️  BLOCKED: Need {shortfall:.6f} more ETH on Base for the fixed protocol fee.")
        print(f"  Current: {eth_bal_eth:.6f} ETH, Need: {FIXED_FEE_WEI/1e18:.4f} ETH")
        if not dry_run:
            print("Re-run with --dry-run to preview the calldata anyway.")
            sys.exit(1)

    if usdc_bal_raw < GIVE_AMOUNT_RAW:
        shortfall_usd = (GIVE_AMOUNT_RAW - usdc_bal_raw) / 1e6
        print(f"\n⚠️  BLOCKED: Need {shortfall_usd:.4f} more USDC on Base.")
        if not dry_run:
            sys.exit(1)

    # Get accurate take amount from API
    print("\nFetching take amount from deBridge API...")
    take_amount = get_take_amount_from_api()

    print(f"\nOrder parameters:")
    print(f"  Give: {GIVE_AMOUNT_RAW/1e6:.4f} USDC (Base)")
    print(f"  Take: {take_amount/1e6:.4f} USDC (Arbitrum)")
    print(f"  Allowed taker: {solver_addr} (ONLY our solver)")
    print(f"  Protocol fee: {FIXED_FEE_WEI/1e18:.4f} ETH")

    # Check USDC allowance
    allowance_raw = int(subprocess.run(
        ['cast', 'call', USDC_BASE,
         f'allowance(address,address)(uint256)', solver_addr, DLN_SOURCE_BASE,
         '--rpc-url', SRC_RPC],
        capture_output=True, text=True
    ).stdout.strip().split('\n')[0] or '0')
    print(f"\nCurrent USDC allowance to DLN_SOURCE: {allowance_raw/1e6:.4f} USDC")

    if do_approve:
        print(f"\nApproving {GIVE_AMOUNT_RAW/1e6:.4f} USDC to DLN_SOURCE ({DLN_SOURCE_BASE})...")
        result = subprocess.run([
            'cast', 'send', USDC_BASE,
            f'approve(address,uint256)', DLN_SOURCE_BASE, str(GIVE_AMOUNT_RAW),
            '--rpc-url', SRC_RPC,
            '--private-key', pk,
        ], capture_output=True, text=True)
        print("stdout:", result.stdout)
        print("stderr:", result.stderr[:200])
        if result.returncode != 0:
            print("❌ Approve failed")
            sys.exit(1)
        print("✅ Approved")
        return

    # Build calldata
    calldata = build_create_salted_order_calldata(solver_addr, take_amount)
    total_value = FIXED_FEE_WEI  # Only the protocol fee as msg.value (USDC transferred via ERC20)
    print(f"\nCalldata: {calldata[:60]}...")
    print(f"msg.value: {total_value} wei ({total_value/1e18:.6f} ETH = fixed protocol fee)")

    if dry_run:
        print("\n[DRY RUN] Simulating on Base...")
        sim = subprocess.run([
            'cast', 'call', DLN_SOURCE_BASE,
            '--rpc-url', SRC_RPC,
            '--from', solver_addr,
            '--value', str(total_value),
            calldata,
        ], capture_output=True, text=True)
        print("Simulation stdout:", sim.stdout[:400])
        print("Simulation stderr:", sim.stderr[:400])
        print("\n[DRY RUN] Done. Remove --dry-run to broadcast.")
        return

    # Broadcast
    if allowance_raw < GIVE_AMOUNT_RAW:
        print(f"\n⚠️  Insufficient allowance! Run with --approve first.")
        sys.exit(1)

    print("\nBroadcasting createSaltedOrder on Base...")
    result = subprocess.run([
        'cast', 'send', DLN_SOURCE_BASE,
        '--rpc-url', SRC_RPC,
        '--private-key', pk,
        '--value', str(total_value),
        '--gas-limit', '400000',
        calldata,
    ], capture_output=True, text=True)
    print("stdout:", result.stdout)
    print("stderr:", result.stderr)

    if result.returncode == 0:
        print("\n✅ Order created! Solver should detect and fill it within ~12s.")
        print("   Monitor with: PROTOCOL_FILTER=debridge DRY_RUN=false ./run-mainnet.sh")
    else:
        print("\n❌ Failed to create order")
        sys.exit(1)


if __name__ == '__main__':
    main()
