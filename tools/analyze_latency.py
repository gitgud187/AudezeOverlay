"""Summarize an existing overlay USB trace; never opens or queries a device."""
import csv
import json
import math
import pathlib
import sys
from collections import Counter


def distribution(values):
    values = sorted(values)
    if not values:
        return {"count": 0}
    return {
        "count": len(values),
        "min_ms": round(values[0], 3),
        "p50_ms": round(values[math.ceil(len(values) * .5) - 1], 3),
        "p95_ms": round(values[math.ceil(len(values) * .95) - 1], 3),
        "p99_ms": round(values[math.ceil(len(values) * .99) - 1], 3),
        "max_ms": round(values[-1], 3),
    }


def analyze(path):
    with path.open(encoding="utf-8-sig", newline="") as source:
        loaded_rows = list(csv.DictReader(source))
    rows = []
    for row in loaded_rows:
        try:
            row["elapsed_ms"] = float(row["elapsed_ms"])
            row["duration_ms"] = float(row["duration_ms"])
        except (TypeError, ValueError):
            # A file still being captured can end with a partial final record.
            continue
        rows.append(row)
    groups = {}
    for row in rows:
        groups.setdefault((row["event"], row["field"]), []).append(row["duration_ms"])
    queries = [r for r in rows if r["event"] == "query" and r["field"] == "Some(Mix)"]
    replies = [r for r in rows if r["event"] == "response" and r["field"] == "Some(Mix)"]
    gaps = [(b["elapsed_ms"] - a["elapsed_ms"], a, b) for a, b in zip(queries, queries[1:])]
    changes = [(a["chat_percent"], b["chat_percent"], b["elapsed_ms"])
               for a, b in zip(replies, replies[1:]) if a["chat_percent"] != b["chat_percent"]]
    requests = []
    active = None
    for row in rows:
        if row["event"] == "query":
            active = {"query": row, "reads": [], "response": None}
            requests.append(active)
        elif active and row["event"] == "get_report":
            active["reads"].append(row)
        elif active and row["event"] == "response":
            active["response"] = row
    mix_requests = [r for r in requests if r["query"]["field"] == "Some(Mix)" and r["reads"]]
    first_read_delays = [r["reads"][0]["elapsed_ms"] - r["reads"][0]["duration_ms"] - r["query"]["elapsed_ms"]
                         for r in mix_requests]
    raw_available = any(r.get("request_seq") for r in rows)
    parsed_mix = [r for r in rows if r["event"] == "parsed_frame" and r["field"] == "Some(Mix)"]
    raw_mix = []
    raw_stream = bytearray()
    raw_byte_times = []
    headers = Counter()
    raw_nonempty = 0
    for row in rows:
        if row["event"] != "get_report" or not row.get("detail"):
            continue
        try:
            packet = bytes.fromhex(row["detail"])
        except ValueError:
            continue
        headers[(packet[0] if packet else -1, packet[2] if len(packet) > 2 else -1)] += 1
        if len(packet) > 1 and packet[1]:
            raw_nonempty += 1
            payload = packet[3:3 + packet[1]]
            raw_stream.extend(payload)
            raw_byte_times.extend([row["elapsed_ms"]] * len(payload))
        for prefix in (bytes.fromhex("05 5b 06 00 83 2c 00 0b 00"), bytes.fromhex("05 5d 06 00 83 2c 00 0b 00")):
            offset = packet.find(prefix)
            while offset >= 0:
                value_at = offset + len(prefix)
                if value_at < len(packet):
                    raw_mix.append((row["elapsed_ms"], packet[value_at]))
                offset = packet.find(prefix, offset + 1)
    reassembled_mix = []
    for prefix in (bytes.fromhex("05 5b 06 00 83 2c 00 0b 00"), bytes.fromhex("05 5d 06 00 83 2c 00 0b 00")):
        offset = raw_stream.find(prefix)
        while offset >= 0:
            value_at = offset + len(prefix)
            if value_at < len(raw_stream):
                reassembled_mix.append((offset, raw_stream[value_at]))
            offset = raw_stream.find(prefix, offset + 1)
    reassembled_mix.sort()
    fragmented_mix = [
        {"start_ms": raw_byte_times[offset], "end_ms": raw_byte_times[offset + 9],
         "assembly_ms": round(raw_byte_times[offset + 9] - raw_byte_times[offset], 3), "chat_percent": raw * 5,
         "stream_offset": offset,
         "first_fragment_bytes": sum(1 for i in range(10) if raw_byte_times[offset + i] == raw_byte_times[offset])}
        for offset, raw in reassembled_mix if raw_byte_times[offset] != raw_byte_times[offset + 9]
    ]
    publishes = {r["delivery_seq"]: r for r in rows if r["event"] == "publish" and r.get("delivery_seq")}
    deliveries = []
    for row in rows:
        if row["event"] not in ("ui_present", "ui_visible") or not row.get("delivery_seq"):
            continue
        publication = publishes.get(row["delivery_seq"])
        deliveries.append({
            "sequence": row["delivery_seq"], "event": row["event"],
            "elapsed_ms": row["elapsed_ms"], "receive_to_present_ms": row["duration_ms"],
            "alpha": row.get("alpha"), "chat_percent": row["chat_percent"],
            "publication_chat": publication["chat_percent"] if publication else None,
            "value_matches": row.get("detail") == "value_matches_delivery",
        })
    # A query can capture an old value immediately before a physical movement.
    # Next-reply minus previous-query measures the trace's sampling/transport
    # blind window, not physical movement latency or a guarantee about firmware.
    blind = []
    for a, b in zip(replies, replies[1:]):
        prior_query = a["elapsed_ms"] - a["duration_ms"]
        blind.append(b["elapsed_ms"] - prior_query)
    largest = []
    for gap, a, b in sorted(gaps, key=lambda x: x[0], reverse=True)[:5]:
        largest.append({"gap_ms": gap, "start_ms": a["elapsed_ms"], "end_ms": b["elapsed_ms"],
                        "events": [r for r in rows if a["elapsed_ms"] <= r["elapsed_ms"] <= b["elapsed_ms"]]})
    return {
        "file": str(path),
        "elapsed_ms": max(r["elapsed_ms"] for r in rows) if rows else 0,
        "rows": len(rows),
        "events": dict(Counter(r["event"] for r in rows)),
        "distributions": {"/".join(k): distribution(v) for k, v in groups.items()},
        "mix_query_gap": distribution([g[0] for g in gaps]),
        "mix_first_read_start_after_query": distribution(first_read_delays),
        "mix_read_count_per_request": dict(Counter(len(r["reads"]) for r in mix_requests)),
        "previous_query_to_next_reply": distribution(blind),
        "mix_changes": changes,
        "parsed_mix_frames": len(parsed_mix),
        "raw_report_headers_id_route": {f"{report:02x}/{route:02x}": count for (report, route), count in headers.items()},
        "raw_nonempty_reports": raw_nonempty,
        "raw_standalone_mix_frames": len(raw_mix),
        "raw_reassembled_mix_frames": len(reassembled_mix),
        "raw_mix_values_match_parsed_sequence": ([str(raw * 5) for _, raw in reassembled_mix] == [r["chat_percent"] for r in parsed_mix]) if raw_available else None,
        "fragmented_mix": fragmented_mix,
        "parsed_mix_changes": [(a["chat_percent"], b["chat_percent"], b["elapsed_ms"])
                               for a, b in zip(parsed_mix, parsed_mix[1:]) if a["chat_percent"] != b["chat_percent"]],
        "ui_deliveries": deliveries,
        "largest_mix_gaps": largest,
        "limitations": ["No physical wheel timestamp in this trace.",
                        "response duration begins after WriteFile, excluding its duration.",
                        "Actual monitor scanout is not recorded; UI samples time UpdateLayeredWindow completion."] + ([
                            "Local request IDs aid analysis but are not present in the device protocol; a late same-field reply after timeout is ambiguous.",
                            "ui_visible means alpha >=230/255; it does not measure visual perception or monitor refresh."] if raw_available else [
                            "Legacy trace: UI records are not sequence-linked to a specific value.",
                            "Legacy trace: UI elapsed_ms is writer drain time; duration_ms is the captured presentation duration.",
                            "Legacy trace: no individual raw frames or request sequence identifiers are recorded."]),
        "incomplete_rows_skipped": len(loaded_rows) - len(rows),
    }


if __name__ == "__main__":
    paths = [pathlib.Path(p) for p in sys.argv[1:]] or [pathlib.Path("dist/usb-timing.csv")]
    print(json.dumps([analyze(p) for p in paths], indent=2))
