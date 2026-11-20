## Earlier findings
- Round 1 findings (three Medium: flag classification, FLUSH ordering, helper/harness): fixed.

No new findings. Kernel claims checked: libublk-rs-sys-0.1.2 ublk_cmd.h (flag bits 8-11,13,15-19, NEED_REG_BUF=1<<17, UBLK_ATTR_*), ublk_drv.c (attrs->blk_queue_write_cache) and block/blk-core.c (clears PREFLUSH/FUA, ends empty flush when no WC) in eve-kernel 6.1. Not verified: 6.8/6.6 sources, tracefs rwbs.

Reviewed commit: 166c0167157527e2ff4b62283b90434b59342603
