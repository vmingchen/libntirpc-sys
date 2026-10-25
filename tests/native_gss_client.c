/* Direct access to the negotiation callbacks permits deterministic failures
 * without a KDC or credentials. */
#include "../vendor/ntirpc/src/auth_gss.c"
#include <assert.h>

static unsigned allocations, releases;

static void *count_alloc(size_t size, const char *file, int line, const char *func)
{
	++allocations;
	return malloc(size);
}

static void *count_calloc(size_t n, size_t size, const char *file, int line,
			  const char *func)
{
	++allocations;
	return calloc(n, size);
}

static void count_free(void *ptr, size_t size, const char *file, int line,
		       const char *func)
{
	if (ptr)
		++releases;
	free(ptr);
}

OM_uint32 gss_init_sec_context(OM_uint32 *minor, gss_cred_id_t cred,
	gss_ctx_id_t *ctx, gss_name_t name, gss_OID mech, OM_uint32 flags,
	OM_uint32 time, gss_channel_bindings_t bindings, gss_buffer_t input,
	gss_OID *actual_mech, gss_buffer_t output, OM_uint32 *ret_flags,
	OM_uint32 *time_rec)
{
	*minor = 0;
	output->value = NULL;
	output->length = 0;
	return GSS_S_FAILURE;
}

int main(void)
{
	__ntirpc_pkg_params.malloc_ = count_alloc;
	__ntirpc_pkg_params.calloc_ = count_calloc;
	__ntirpc_pkg_params.free_size_ = count_free;
	struct rpc_gss_sec sec = {.svc = RPCSEC_GSS_SVC_NONE};
	for (int i = 0; i < 10; ++i)
		assert(authgss_ncreate(NULL, GSS_C_NO_NAME, &sec) == NULL);
	assert(allocations == 10 && releases == 10);

	struct rpc_gss_data *gd = mem_zalloc(sizeof(*gd));
	mutex_init(&gd->lock, NULL);
	gd->gc.gc_proc = RPCSEC_GSS_INIT;
	struct opaque_auth verifier = {.oa_length = 4};
	memcpy(verifier.oa_body, "test", 4);
	for (int i = 0; i < 10; ++i)
		assert(authgss_validate(&gd->gd_auth, &verifier));
	assert(allocations - releases == 2); /* gd and the latest verifier */
	authgss_destroy(&gd->gd_auth);
	assert(allocations == releases);
	return 0;
}
