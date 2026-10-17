#include <assert.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <sys/un.h>
#include <rpc/rpc.h>

static size_t tracked_size;
static unsigned char *tracked_allocation;

static void *guarded_calloc(size_t count, size_t size,
			   const char *file, int line, const char *function)
{
	(void)file;
	(void)line;
	(void)function;
	assert(count == 1);
	assert(tracked_allocation == NULL);
	tracked_size = size;
	tracked_allocation = calloc(1, size + 64);
	assert(tracked_allocation != NULL);
	memset(tracked_allocation + size, 0xa5, 64);
	return tracked_allocation;
}

static struct netconfig local_config(void)
{
	struct netconfig config = {0};
	config.nc_netid = "local";
	config.nc_semantics = NC_TPI_COTS_ORD;
	return config;
}

static void long_unix_address(size_t length)
{
	struct netconfig config = local_config();
	struct sockaddr_un address = {.sun_family = AF_UNIX};
	struct netbuf buffer = {0};
	address.sun_path[0] = '/';
	memset(address.sun_path + 1, 'a', length - 1);
	buffer.buf = &address;
	buffer.len = offsetof(struct sockaddr_un, sun_path) + length + 1;
	buffer.maxlen = sizeof(address);

	/* This isolated process has no worker threads. Extra guard storage makes
	 * the old undersized allocation observable without an out-of-bounds write. */
	mem_2_size_t saved = __ntirpc_pkg_params.calloc_;
	__ntirpc_pkg_params.calloc_ = guarded_calloc;
	char *result = taddr2uaddr(&config, &buffer);
	__ntirpc_pkg_params.calloc_ = saved;
	assert((void *)result == tracked_allocation);
	assert(tracked_size >= length + 1);
	for (size_t i = 0; i < 64; ++i)
		assert(tracked_allocation[tracked_size + i] == 0xa5);
	assert(strlen(result) == length);
	assert(memcmp(result, address.sun_path, length) == 0);
	mem_free(result, 0);
	tracked_allocation = NULL;
}

static void unix_address_respects_length(void)
{
	struct netconfig config = local_config();
	struct sockaddr_un address = {.sun_family = AF_UNIX};
	memcpy(address.sun_path, "/okSECRET", 10);
	struct netbuf buffer = {
		.buf = &address,
		.len = offsetof(struct sockaddr_un, sun_path) + 3,
		.maxlen = sizeof(address)
	};
	char *result = taddr2uaddr(&config, &buffer);
	assert(result != NULL);
	assert(strcmp(result, "/ok") == 0);
	mem_free(result, 0);

	buffer.len = offsetof(struct sockaddr_un, sun_path) - 1;
	assert(taddr2uaddr(&config, &buffer) == NULL);
	buffer.len = offsetof(struct sockaddr_un, sun_path);
	result = taddr2uaddr(&config, &buffer);
	assert(result != NULL && result[0] == '\0');
	mem_free(result, 0);
}

int main(void)
{
	long_unix_address(59);
	long_unix_address(107);
	unix_address_respects_length();
	return 0;
}
