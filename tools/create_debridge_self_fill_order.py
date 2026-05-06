#!/usr/bin/env python3
"""
Create a tiny deBridge DLN self-fill order for e2e testing.

Creates a Base→Arbitrum ETH order where the solver itself is the allowedTakerDst.
The solver will auto-detect and fill this via its eth_getLogs poller.

Usage:
    python3 tools/create_debridge_self_fill_order.py [--dry-run]

Environment:
    SOLVER_PRIVATE_KEY   EVM private key (or loaded from macOS keychain 'mamba-messiah-key')
"""

import subprocess
import os
import sys
import json
import time

# === Config ===
# Create order on Arbitrum (we have 0.002263 ETH there)
# Fill order on Base (we have 0.001444 ETH there)
# Fixed native fee on Arbitrum DLN = 0.001 ETH
# Total from Arbitrum: GIVE_AMOUNT + 0.001 ETH fee + gas (~0.0001 ETH)
# We have 0.002263 ETH on Arbitrum → budget for give = 0.002263 - 0.001 - 0.0002 = 0.000763 ETH
GIVE_AMOUNT_WEI   = 600_000_000_000_000    # 0.0006 ETH on Arbitrum (given to escrow)
TAKE_AMOUNT_WEI   = 550_000_000_000_000    # 0.00055 ETH on Base (solver fills with this)
TAKE_CHAIN_ID     = 8453                   # Base (dst, where solver fills)
SRC_RPC           = "https://arb1.arbitrum.io/rpc"
DLN_SOURCE_ARB    = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66"

def abi_encode_bytes(data: bytes) -> bytes:
    """ABI-encode a bytes value (length + padded data)."""
    length = len(data)
    padded_len = (length + 31) // 32 * 32
    return length.to_bytes(32, 'big') + data + b'\x00' * (padded_len - length)

def get_solver_address(private_key: str) -> str:
    r = subprocess.run(['cast', 'wallet', 'address', private_key], capture_output=True, text=True)
    return r.stdout.strip()

def build_create_salted_order_calldata(solver_addr: str) -> str:
    """Build createSaltedOrder calldata with solver as allowedTakerDst."""
    import hashlib

    solver_bytes = bytes.fromhex(solver_addr.lstrip('0x'))  # 20 bytes
    zero_addr = bytes(20)  # native ETH = 0x0000...
    take_token = bytes(20)  # native ETH on Arbitrum

    # OrderCreation struct fields (in order per DlnOrderLib.sol):
    # address giveTokenAddress  (static, 32 bytes)
    # uint256 giveAmount        (static, 32 bytes)
    # bytes takeTokenAddress    (dynamic)
    # uint256 takeAmount        (static, 32 bytes)
    # uint256 takeChainId       (static, 32 bytes)
    # bytes receiverDst         (dynamic)
    # address givePatchAuthoritySrc (static, 32 bytes)
    # bytes orderAuthorityAddressDst (dynamic)
    # bytes allowedTakerDst     (dynamic)
    # bytes externalCall        (dynamic)
    # bytes allowedCancelBeneficiarySrc (dynamic)

    # Static fields:
    f_give_token = zero_addr.rjust(32, b'\x00')                        # 32 bytes
    f_give_amount = GIVE_AMOUNT_WEI.to_bytes(32, 'big')                # 32 bytes
    f_take_amount = TAKE_AMOUNT_WEI.to_bytes(32, 'big')                # 32 bytes
    f_take_chain = TAKE_CHAIN_ID.to_bytes(32, 'big')                   # 32 bytes
    f_patch_auth = solver_bytes.rjust(32, b'\x00')                     # 32 bytes (address)

    # Dynamic fields (will be placed after all heads)
    dyn_take_token = take_token                                          # 20 bytes
    dyn_receiver = solver_bytes                                          # 20 bytes (receive on Arbitrum)
    dyn_order_auth = solver_bytes                                        # 20 bytes
    dyn_allowed_taker = solver_bytes                                     # 20 bytes (ONLY we can fill!)
    dyn_external_call = b''                                              # empty
    dyn_cancel_beneficiary = b''                                        # empty

    # Count static head slots: give_token(1) + give_amount(1) + offset(1) + take_amount(1) + take_chain(1) +
    #                           offset(1) + patch_auth(1) + offset(1) + offset(1) + offset(1) + offset(1) = 11 static head slots
    n_static = 11
    static_size = n_static * 32  # bytes used by static part of struct

    def dyn_offset(prev_dyn_sizes, start_offset=static_size):
        return start_offset + sum(prev_dyn_sizes)

    dyn_take_token_enc = abi_encode_bytes(dyn_take_token)     # 32+32 = 64 bytes
    dyn_receiver_enc   = abi_encode_bytes(dyn_receiver)       # 32+32 = 64 bytes
    dyn_order_auth_enc = abi_encode_bytes(dyn_order_auth)     # 32+32 = 64 bytes
    dyn_allowed_enc    = abi_encode_bytes(dyn_allowed_taker)  # 32+32 = 64 bytes
    dyn_ext_enc        = abi_encode_bytes(dyn_external_call)  # 32 bytes (empty)
    dyn_cancel_enc     = abi_encode_bytes(dyn_cancel_beneficiary)  # 32 bytes (empty)

    # Offsets relative to start of struct data:
    off_take_token   = dyn_offset([])
    off_receiver     = dyn_offset([len(dyn_take_token_enc)])
    off_patch_auth_static = None  # address is static (not offset)
    off_order_auth   = dyn_offset([len(dyn_take_token_enc), len(dyn_receiver_enc)])
    off_allowed      = dyn_offset([len(dyn_take_token_enc), len(dyn_receiver_enc), len(dyn_order_auth_enc)])
    off_ext          = dyn_offset([len(dyn_take_token_enc), len(dyn_receiver_enc), len(dyn_order_auth_enc), len(dyn_allowed_enc)])
    off_cancel       = dyn_offset([len(dyn_take_token_enc), len(dyn_receiver_enc), len(dyn_order_auth_enc), len(dyn_allowed_enc), len(dyn_ext_enc)])

    # Build struct head (static slots + offsets for dynamic fields):
    struct_head = b''
    struct_head += f_give_token              # giveTokenAddress (static)
    struct_head += f_give_amount             # giveAmount (static)
    struct_head += off_take_token.to_bytes(32, 'big')   # offset to takeTokenAddress
    struct_head += f_take_amount             # takeAmount (static)
    struct_head += f_take_chain              # takeChainId (static)
    struct_head += off_receiver.to_bytes(32, 'big')     # offset to receiverDst
    struct_head += f_patch_auth              # givePatchAuthoritySrc (static address)
    struct_head += off_order_auth.to_bytes(32, 'big')   # offset to orderAuthorityAddressDst
    struct_head += off_allowed.to_bytes(32, 'big')      # offset to allowedTakerDst
    struct_head += off_ext.to_bytes(32, 'big')          # offset to externalCall
    struct_head += off_cancel.to_bytes(32, 'big')       # offset to allowedCancelBeneficiarySrc

    struct_tail = (dyn_take_token_enc + dyn_receiver_enc + dyn_order_auth_enc +
                   dyn_allowed_enc + dyn_ext_enc + dyn_cancel_enc)

    struct_encoded = struct_head + struct_tail

    # Top-level createSaltedOrder params:
    # 0: offset to OrderCreation struct
    # 1: salt (uint64) — use current timestamp
    # 2: offset to affiliateFee (bytes, empty)
    # 3: referralCode (uint32) = 0
    # 4: offset to permitEnvelope (bytes, empty)
    # 5: offset to metadata (bytes, empty)

    n_top_slots = 6
    top_static_size = n_top_slots * 32

    salt = int(time.time()) & 0xFFFFFFFFFFFFFFFF  # uint64 from current unix timestamp

    # Dynamic offsets in top-level (relative to start of calldata):
    off_struct = top_static_size                                          # struct starts right after 6 slots
    off_affiliate = top_static_size + len(struct_encoded)                # empty bytes after struct
    off_permit = off_affiliate + 32                                       # empty bytes: just length=0 (32 bytes)
    off_metadata = off_permit + 32                                        # empty bytes: just length=0 (32 bytes)

    top_head = b''
    top_head += off_struct.to_bytes(32, 'big')              # offset to struct
    top_head += salt.to_bytes(32, 'big')                    # salt (uint64, padded to 32)
    top_head += off_affiliate.to_bytes(32, 'big')           # offset to affiliateFee
    top_head += (0).to_bytes(32, 'big')                     # referralCode (uint32) = 0
    top_head += off_permit.to_bytes(32, 'big')              # offset to permitEnvelope
    top_head += off_metadata.to_bytes(32, 'big')            # offset to metadata

    top_tail = (struct_encoded +
                abi_encode_bytes(b'') +   # affiliateFee = empty
                abi_encode_bytes(b'') +   # permitEnvelope = empty
                abi_encode_bytes(b''))    # metadata = empty

    # Function selector for createSaltedOrder
    selector = bytes.fromhex('b9303701')

    calldata = selector + top_head + top_tail
    return '0x' + calldata.hex()


def main():
    dry_run = '--dry-run' in sys.argv

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
    print(f"Create order: {GIVE_AMOUNT_WEI/1e18:.6f} ETH on Base → {TAKE_AMOUNT_WEI/1e18:.6f} ETH on Arbitrum")
    print(f"Allowed taker: {solver_addr} (ONLY our solver can fill this)")

    calldata = build_create_salted_order_calldata(solver_addr)
    print(f"\nCalldata: {calldata[:60]}...")
    print(f"Value: {GIVE_AMOUNT_WEI} wei ({GIVE_AMOUNT_WEI/1e18:.6f} ETH)")

    if dry_run:
        print("\n[DRY RUN] — calldata built. Pass --no-dry-run to broadcast.")
        # Simulate on Arbitrum
        fixed_fee = 1_000_000_000_000_000  # 0.001 ETH fixed deBridge fee
        total_value = GIVE_AMOUNT_WEI + fixed_fee
        print(f"msg.value = {total_value} wei ({total_value/1e18:.6f} ETH) = give + fee")
        sim = subprocess.run([
            'cast', 'call',
            DLN_SOURCE_ARB,
            '--rpc-url', SRC_RPC,
            '--from', solver_addr,
            '--value', str(total_value),
            calldata,
        ], capture_output=True, text=True)
        print("Simulation stdout:", sim.stdout[:300])
        print("Simulation stderr:", sim.stderr[:300])
        return

    # Broadcast
    print("\nBroadcasting createSaltedOrder on Arbitrum...")
    fixed_fee = 1_000_000_000_000_000  # 0.001 ETH fixed deBridge fee
    total_value = GIVE_AMOUNT_WEI + fixed_fee
    print(f"msg.value = {total_value} wei ({total_value/1e18:.6f} ETH) = give + fee")
    result = subprocess.run([
        'cast', 'send',
        DLN_SOURCE_ARB,
        '--rpc-url', SRC_RPC,
        '--private-key', pk,
        '--value', str(total_value),
        '--gas-limit', '400000',
        calldata,
    ], capture_output=True, text=True)
    print("stdout:", result.stdout)
    print("stderr:", result.stderr)

    if result.returncode == 0:
        print("\n✅ Order created! The solver should detect and fill it within ~12 seconds.")
    else:
        print("\n❌ Failed to create order")
        sys.exit(1)


if __name__ == '__main__':
    main()
