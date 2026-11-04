# Steady state (from 30 s): flush latency, the gap from a flush's completion to the next flush's
# issue, and the writes issued and in flight during each phase. Flushes are matched in order.
$1 == "I" || $1 == "C" { t = $2 / 1e9; if (t0 == "") t0 = t }
t - t0 < 30 { next }
$1 == "I" && $3 ~ /^W/ { inflight[$4] = 1; nw++; if (fl) { during_flush++; dbytes += $5 } else { gap_bytes += $5 } }
$1 == "C" && $3 ~ /^W/ { delete inflight[$4] }
$1 == "I" && $3 == "FF" { fi = t; n = 0; for (s in inflight) n++; atissue += n; if (lastc != "") { gap += t - lastc; ng++ } fl = 1; nf++ }
$1 == "C" && $3 == "FF" { if (fi != "") { lat += t - fi; nl++ } lastc = t; fl = 0 }
END {
    printf "flushes %d: latency %.2f ms; gap to the next flush %.2f ms; writes in flight at a flush's issue %.1f\n", nf, lat / nl * 1000, gap / ng * 1000, atissue / nf
    printf "write MiB issued during flushes %.0f, between flushes %.0f (per flush: %.2f and %.2f MiB)\n", dbytes / 1048576, gap_bytes / 1048576, dbytes / 1048576 / nf, gap_bytes / 1048576 / nf
}
