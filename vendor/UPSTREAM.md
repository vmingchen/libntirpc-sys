# Bundled ntirpc

Source: https://github.com/nfs-ganesha/ntirpc

Tag: v15.7

Commit: 69c40fecf260ff9172f6dd9d7b468b0a541808bc

This revision contains PR #414 (merge commit
e991cea4134fff07ceb6ec11ef82addf0679bb43), which copies the decoded reply
verifier before AUTH_VALIDATE. Cargo builds use packaged source, never git
clone or a network download.

Two local patches are maintained:

- src/CMakeLists.txt: build a PIC static archive rather than a shared library,
  avoiding an accidental runtime dependency on a different system ntirpc.
- src/auth_gss.c: use mem_zalloc for rpc_gss_data. Upstream v15.7 still leaves
  credential fields uninitialized with mem_alloc. This initializes the object
  locally through upstream's allocator API, replacing the process-global
  allocator-hook workaround. This patch should be proposed upstream separately.

Upstream license notices are retained in vendor/ntirpc/COPYING and source files.
Monitoring, LTTng, RDMA and TLS are disabled; IPv6 matches upstream defaults.
RPCSEC_GSS is compiled only when the rpcsec-gss Cargo feature is enabled.
