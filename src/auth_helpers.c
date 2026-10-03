#include <stddef.h>
#include <rpc/auth.h>
#include <rpc/svc.h>
#ifdef VFSI_RPCSEC_GSS
#include <rpc/auth_gss.h>
#endif

void vfsi_libntirpc_auth_destroy(AUTH *auth) { auth_destroy(auth); }
void vfsi_libntirpc_set_process_cb(SVCXPRT *xprt, svc_req_fun_t callback) {
    xprt->xp_dispatch.process_cb = callback;
}
size_t vfsi_libntirpc_sizeof_svcxprt(void) { return sizeof(SVCXPRT); }
#ifdef VFSI_RPCSEC_GSS
AUTH *vfsi_libntirpc_authgss_ncreate_default(CLIENT *client, char *service,
                                           struct rpc_gss_sec *security) {
    return authgss_ncreate_default(client, service, security);
}
#endif
