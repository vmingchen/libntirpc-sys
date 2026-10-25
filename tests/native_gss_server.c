#include "../vendor/ntirpc/src/svc_auth_gss.c"
#include "../vendor/ntirpc/src/svc_internal.h"
#include <assert.h>

enum failure { DISPLAY, MIC, LONG_MIC, REPLY_COMPLETE, REPLY_CONTINUE };
static enum failure failure;
static gss_union_ctx_id_desc context;
static void *context_copy;
static unsigned destroyed;

static void *track_alloc(size_t size, const char *file, int line, const char *func)
{
	void *ptr = malloc(size);
	if (strcmp(func, "svcauth_gss_accept_sec_context") == 0) {
		assert(context_copy == NULL);
		context_copy = ptr;
	}
	return ptr;
}

static void track_free(void *ptr, size_t size, const char *file, int line,
		       const char *func)
{
	if (ptr == context_copy)
		context_copy = NULL;
	free(ptr);
}

OM_uint32 gss_accept_sec_context(OM_uint32 *minor, gss_ctx_id_t *ctx,
	gss_cred_id_t cred, gss_buffer_t input, gss_channel_bindings_t bindings,
	gss_name_t *name, gss_OID *mech, gss_buffer_t output, OM_uint32 *flags,
	OM_uint32 *time, gss_cred_id_t *delegated)
{
	*minor = 0;
	*ctx = (gss_ctx_id_t)&context;
	*name = GSS_C_NO_NAME;
	*mech = GSS_C_NO_OID;
	*flags = 0;
	*time = 3600;
	output->value = NULL;
	output->length = 0;
	return failure == REPLY_CONTINUE ? GSS_S_CONTINUE_NEEDED : GSS_S_COMPLETE;
}

OM_uint32 gss_display_name(OM_uint32 *minor, gss_name_t name,
			  gss_buffer_t output, gss_OID *type)
{
	*minor = 0;
	output->value = NULL;
	output->length = 0;
	return failure == DISPLAY ? GSS_S_FAILURE : GSS_S_COMPLETE;
}

OM_uint32 gss_get_mic(OM_uint32 *minor, gss_ctx_id_t ctx, gss_qop_t qop,
		     gss_buffer_t input, gss_buffer_t output)
{
	*minor = 0;
	if (failure == MIC)
		return GSS_S_FAILURE;
	output->length = failure == LONG_MIC ? MAX_AUTH_BYTES + 1 : 4;
	output->value = calloc(1, output->length);
	return GSS_S_COMPLETE;
}

OM_uint32 gss_delete_sec_context(OM_uint32 *minor, gss_ctx_id_t *ctx,
				gss_buffer_t output)
{
	*minor = 0;
	*ctx = GSS_C_NO_CONTEXT;
	++destroyed;
	return GSS_S_COMPLETE;
}

static enum xprt_stat fail_reply(struct svc_req *req)
{
	return XPRT_DIED;
}

static void failed_negotiation(enum failure which)
{
	failure = which;
	struct svc_req req = {0};
	struct xp_ops ops = {.xp_reply = fail_reply};
	SVCXPRT xprt = {.xp_ops = &ops};
	req.rq_xprt = &xprt;
	struct rpc_gss_cred cred = {
		.gc_v = RPCSEC_GSS_VERSION,
		.gc_proc = RPCSEC_GSS_INIT,
		.gc_svc = RPCSEC_GSS_SVC_NONE,
	};
	XDR stream;
	xdrmem_create(&stream, req.rq_msg.cb_cred.oa_body, MAX_AUTH_BYTES, XDR_ENCODE);
	assert(xdr_rpc_gss_cred(&stream, &cred));
	req.rq_msg.cb_cred.oa_length = XDR_GETPOS(&stream);
	xdr_destroy(&stream);
	uint32_t empty_token = 0;
	xdrmem_create(&stream, (char *)&empty_token, sizeof(empty_token), XDR_DECODE);
	req.rq_xdrs = &stream;
	bool no_dispatch = false;
	enum auth_stat status = _svcauth_gss(&req, &no_dispatch);
	assert(status == (which >= REPLY_COMPLETE ? AUTH_FAILED : AUTH_REJECTEDCRED));
	assert(req.rq_auth == NULL);
	assert(context_copy == NULL);
	xdr_destroy(&stream);
}

int main(int argc, char **argv)
{
	__svc_params->gss.ctx_hash_partitions = 13;
	server_creds.gss_creds = (gss_cred_id_t)&context;
	server_creds.gss_creds_expires = 0;
	__ntirpc_pkg_params.malloc_ = track_alloc;
	__ntirpc_pkg_params.free_size_ = track_free;
	int first = argc > 1 ? atoi(argv[1]) : DISPLAY;
	int last = argc > 1 ? first : REPLY_CONTINUE;
	for (int i = first; i <= last; ++i) {
		failed_negotiation(i);
		assert(destroyed == i - first + 1);
	}
	return 0;
}
