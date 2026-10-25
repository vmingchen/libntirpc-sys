# Bundled ntirpc

Source: https://github.com/nfs-ganesha/ntirpc

Base release: v15.7 (plus upstream PR #416)

Commit: ff0ee01fc4960a72f70f21ca6a778c0963732a7b

This revision contains PR #414 (merge commit
e991cea4134fff07ceb6ec11ef82addf0679bb43), which copies the decoded reply
verifier before AUTH_VALIDATE, and PR #416 (merge commit
ff0ee01fc4960a72f70f21ca6a778c0963732a7b), which zero-initializes rpc_gss_data
through mem_zalloc. Both runtime fixes are upstream. Cargo builds use packaged
source, never git clone or a network download.

One local build patch is maintained:

- src/CMakeLists.txt: build a PIC static archive rather than a shared library,
  avoiding an accidental runtime dependency on a different system ntirpc.

Upstream license notices are retained in vendor/ntirpc/COPYING and source files.
Monitoring, LTTng, RDMA and TLS are disabled; IPv6 matches upstream defaults.
RPCSEC_GSS is compiled only when the rpcsec-gss Cargo feature is enabled.

Additional correctness patches are recorded in [PATCHES.md](PATCHES.md).
Reviewed open upstream PR commits are backported on top of this base pin;
their exact revisions, adjustments, and exclusions are recorded there.
