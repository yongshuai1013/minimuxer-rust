#!/usr/bin/env python3
"""Generate the Swift shell's MmErrorInfo -> Swift error conversion tables
from the single source of truth: crates/mm-common/error-map.toml.

Outputs (checked into the tree; CI re-runs this script and diffs):
  Common/Sources/MmErrorInfo+PairingError.swift        (module MinimuxerCommon)
  DeviceGateway/Sources/MmErrorInfo+DeviceGatewayError.swift (module DeviceGatewayAPI)
  Shell/Sources/MmErrorInfo+MinimuxerError.swift       (module Minimuxer)

Usage:
  generate-error-table.py           write the files in place
  generate-error-table.py --check   exit 1 if any file drifts from the toml
"""
import re
import sys
from pathlib import Path

try:
    import tomllib
except ImportError:  # python < 3.11 (e.g. /usr/bin/python3 on macOS)
    tomllib = None


def load_cases(path):
    """Return the [[case]] entries of error-map.toml as dicts.
    Uses tomllib when available; otherwise a restricted parser for
    this file's shape (flat `key = "value"` lines inside [[case]])."""
    if tomllib is not None:
        with open(path, "rb") as f:
            return tomllib.load(f)["case"]
    cases = []
    current = None
    for line in path.read_text().splitlines():
        line = line.strip()
        if line == "[[case]]":
            current = {}
            cases.append(current)
        elif current is not None:
            m = re.match(r'^(\w+)\s*=\s*"(.*)"$', line)
            if m:
                current[m.group(1)] = m.group(2).replace('\\"', '"').replace("\\\\", "\\")
    return cases

ROOT = Path(__file__).resolve().parent.parent
TOML_PATH = ROOT / "crates" / "mm-common" / "error-map.toml"

HEADER = """\
// GENERATED from crates/mm-common/error-map.toml by
// Scripts/generate-error-table.py -- do not edit by hand.
// Regenerate: python3 Scripts/generate-error-table.py
"""

SEP = "\u0001"  # U+0001 payload separator used inside MmErrorInfo.reason
SWIFT_SEP = '"\\u{1}"'  # Swift source literal for the separator string


def payload_kind(case):
    """Derive the payload shape from the description template, mirroring
    the encoding contract documented in error-map.toml's header."""
    desc = case["description"]
    if "{protocol}" in desc and "{missing_keys}" in desc:
        return "protocol_keys"      # PairingError.incomplete
    if "{protocol}" in desc:
        return "protocol_reason"    # MinimuxerError.invalidPairing / .mount
    if "{code}" in desc:
        return "i32"                # EMProxyError.unknownError
    if "{reason}" in desc:
        return "string"
    return "none"


def swift_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def gen_pairing(cases):
    lines = [HEADER, "import Foundation", "import MinimuxerRustBindings", ""]
    lines.append("/// Rebuild a `PairingError` from its FFI representation.")
    lines.append("public func mmPairingError(from info: MmErrorInfo) -> PairingError {")
    lines.append("    let code = info.code.toString()")
    lines.append("    let reason = info.reason.toString()")
    lines.append("    switch code {")
    for c in cases:
        kind = payload_kind(c)
        name = c["code"]
        if kind == "protocol_keys":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        let parts = reason.components(separatedBy: {SWIFT_SEP})")
            lines.append("        let proto = PairingProtocol(rawValue: parts.first ?? \"\") ?? .unknown")
            lines.append("        let keys = parts.count > 1 && !parts[1].isEmpty ? parts[1].components(separatedBy: \",\") : []")
            lines.append(f"        return .{name}(protocolType: proto, missingKeys: keys)")
        elif kind == "string":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}(reason)")
        elif kind == "none":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}")
        else:
            raise SystemExit(f"PairingError case {name}: unexpected payload kind {kind}")
    lines.append("    default:")
    lines.append("        return .invalidPlist(reason)")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def gen_gateway(cases):
    # DeviceGatewayError is a class keyed by a string Code; reconstruction
    # is uniform for every case in the domain (code + reason pass through).
    codes = ", ".join(c["code"] for c in cases)
    lines = [HEADER, "import Foundation", "import MinimuxerRustBindings", ""]
    lines.append(f"// Domain cases (from error-map.toml): {codes}")
    lines.append("/// Rebuild a `DeviceGatewayError` from its FFI representation.")
    lines.append("public func mmDeviceGatewayError(from info: MmErrorInfo) -> DeviceGatewayError {")
    lines.append("    DeviceGatewayError(DeviceGatewayError.Code(info.code.toString()), reason: info.reason.toString())")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def gen_minimuxer(mm_cases, emp_cases, internal_cases, service_cases):
    lines = [HEADER, "import Foundation", "import MinimuxerCommon",
             "import DeviceGatewayAPI", "import MinimuxerRustBindings", ""]
    # --- MinimuxerError ---
    lines.append("/// Rebuild a `MinimuxerError` from its FFI representation.")
    lines.append("public func mmMinimuxerError(from info: MmErrorInfo) -> MinimuxerError {")
    lines.append("    let code = info.code.toString()")
    lines.append("    let reason = info.reason.toString()")
    lines.append("    switch code {")
    for c in mm_cases:
        kind = payload_kind(c)
        name = c["code"]
        if kind == "protocol_reason":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        let parts = reason.components(separatedBy: {SWIFT_SEP})")
            lines.append("        let proto = PairingProtocol(rawValue: parts.first ?? \"\") ?? .unknown")
            lines.append("        let rest = parts.count > 1 ? parts[1] : \"\"")
            lines.append(f"        return .{name}(protocol: proto, reason: rest)")
        elif kind == "string":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}(reason)")
        else:
            raise SystemExit(f"MinimuxerError case {name}: unexpected payload kind {kind}")
    lines.append("    default:")
    lines.append("        return .noConnection(reason)")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    # --- EMProxyError ---
    lines.append("/// Rebuild an `EMProxyError` from its FFI representation.")
    lines.append("public func mmEMProxyError(from info: MmErrorInfo) -> EMProxyError {")
    lines.append("    let code = info.code.toString()")
    lines.append("    let reason = info.reason.toString()")
    lines.append("    switch code {")
    for c in emp_cases:
        kind = payload_kind(c)
        name = c["code"]
        if kind == "i32":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}(Int32(reason) ?? 0)")
        elif kind == "string":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}(reason)")
        elif kind == "none":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}")
        else:
            raise SystemExit(f"EMProxyError case {name}: unexpected payload kind {kind}")
    lines.append("    default:")
    lines.append("        return .unknownError(0)")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    # --- MinimuxerInternalError ---
    lines.append("/// Rebuild a `MinimuxerInternalError` from its FFI representation.")
    lines.append("func mmInternalError(from info: MmErrorInfo) -> MinimuxerInternalError {")
    lines.append("    let code = info.code.toString()")
    lines.append("    let reason = info.reason.toString()")
    lines.append("    switch code {")
    for c in internal_cases:
        kind = payload_kind(c)
        name = c["code"]
        if kind == "string":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}(reason)")
        elif kind == "none":
            lines.append(f"    case {swift_str(name)}:")
            lines.append(f"        return .{name}")
        else:
            raise SystemExit(f"MinimuxerInternalError case {name}: unexpected payload kind {kind}")
    lines.append("    default:")
    lines.append("        return .pairingFailed(reason)")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    # --- MinimuxerServiceError ---
    comps = ", ".join(c["code"] for c in service_cases)
    lines.append(f"// MinimuxerServiceError: MmErrorInfo.code carries the component raw")
    lines.append(f"// value ({comps}); reason carries the inner error's description.")
    lines.append("/// Rebuild a `MinimuxerServiceError` from its FFI representation.")
    lines.append("public func mmServiceError(from info: MmErrorInfo) -> MinimuxerServiceError {")
    lines.append("    let component = MinimuxerComponent(rawValue: info.code.toString()) ?? .heartbeat")
    lines.append("    let inner = NSError(domain: \"MinimuxerServiceError.inner\", code: 0,")
    lines.append("                        userInfo: [NSLocalizedDescriptionKey: info.reason.toString()])")
    lines.append("    return MinimuxerServiceError(component: component, error: inner)")
    lines.append("}")
    lines.append("")
    # --- Dispatcher ---
    lines.append("/// Full-domain dispatcher used by the shell's Minimuxer module:")
    lines.append("/// rebuilds the Swift error type named by `MmErrorInfo.domain`.")
    lines.append("/// Unknown domains keep their Rust-rendered description")
    lines.append("/// (`mm_describe_error`) inside an NSError instead of inventing a case.")
    lines.append("public func mmSwiftError(from info: MmErrorInfo) -> Error {")
    lines.append("    switch info.domain.toString() {")
    lines.append("    case \"MinimuxerError\": return mmMinimuxerError(from: info)")
    lines.append("    case \"DeviceGatewayError\": return mmDeviceGatewayError(from: info)")
    lines.append("    case \"PairingError\": return mmPairingError(from: info)")
    lines.append("    case \"EMProxyError\": return mmEMProxyError(from: info)")
    lines.append("    case \"MinimuxerInternalError\": return mmInternalError(from: info)")
    lines.append("    case \"MinimuxerServiceError\": return mmServiceError(from: info)")
    lines.append("    default:")
    lines.append("        return NSError(domain: info.domain.toString(), code: 0,")
    lines.append("                       userInfo: [NSLocalizedDescriptionKey: mm_describe_error(info).toString()])")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def main():
    check = "--check" in sys.argv
    by_domain = {}
    for case in load_cases(TOML_PATH):
        by_domain.setdefault(case["domain"], []).append(case)

    outputs = {
        ROOT / "Common/Sources/MmErrorInfo+PairingError.swift":
            gen_pairing(by_domain["PairingError"]),
        ROOT / "DeviceGateway/Sources/MmErrorInfo+DeviceGatewayError.swift":
            gen_gateway(by_domain["DeviceGatewayError"]),
        ROOT / "Shell/Sources/MmErrorInfo+MinimuxerError.swift":
            gen_minimuxer(by_domain["MinimuxerError"], by_domain["EMProxyError"],
                          by_domain["MinimuxerInternalError"], by_domain["MinimuxerServiceError"]),
    }

    drift = False
    for path, content in outputs.items():
        if check:
            current = path.read_text() if path.exists() else None
            if current != content:
                print(f"DRIFT: {path.relative_to(ROOT)}")
                drift = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
            print(f"wrote {path.relative_to(ROOT)}")
    if check and drift:
        sys.exit(1)
    if check:
        print("error tables in sync with error-map.toml")


if __name__ == "__main__":
    main()
