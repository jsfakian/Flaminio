# Per flush cycle (from a disk flush's completion to the next one's), from 30 s on: when the
# ublk writes (U) and the disk's writes (D) are issued and completed, relative to the cycle's
# start, in microseconds. Lines: I|C TIME U|D RWBS SECTOR BYTES. A trace without U/D (raw)
# can be given with dev=D on every line by: awk '{ $3 = "D " $3; print }'.
$1 == "I" || $1 == "C" { t = $2 / 1e3 } # us
$1 == "I" || $1 == "C" { if (t0 == "") t0 = t }
t - t0 < 30e6 { if ($1 == "C" && $3 == "D" && $4 == "FF") start = t; next }
$1 == "C" && $3 == "D" && $4 == "FF" {
    if (start != "" && nfl) { cycles++; for (k in cur) { if (cur[k] != "") { sum[k] += cur[k]; cnt[k]++ } } ; for (k in n) { nsum[k] += n[k] } }
    delete cur; delete n; start = t; nfl = 1; next
}
start == "" { next }
function first(k, v) { if (cur["first " k] == "") cur["first " k] = v }
function last(k, v) { cur["last " k] = v }
{ dt = t - start }
$1 == "I" && $3 == "U" && $4 ~ /^W/ { first("U issue", dt); last("U issue", dt); n["U writes"]++ }
$1 == "C" && $3 == "U" && $4 ~ /^W/ { first("U complete", dt); last("U complete", dt) }
$1 == "I" && $3 == "D" && $4 ~ /^W/ { first("D issue", dt); last("D issue", dt); n["D MiB"] += $6 / 1048576 }
$1 == "C" && $3 == "D" && $4 ~ /^W/ { last("D complete", dt) }
$1 == "I" && $3 == "D" && $4 == "FF" { cur["flush issue"] = dt }
END {
    printf "cycles %d, %.0f us each on average\n", cycles, sum["flush issue"] / cnt["flush issue"] + 0
    split("first U complete|last U complete|first U issue|last U issue|first D issue|last D issue|last D complete|flush issue", ks, "|")
    for (i = 1; i in ks; i++) if (cnt[ks[i]]) printf "  %-16s %7.0f us\n", ks[i], sum[ks[i]] / cnt[ks[i]]
    for (k in nsum) printf "  %-16s %7.2f per cycle\n", k, nsum[k] / cycles
}
