#include <assert.h>
#include <stddef.h>
#include <stdio.h>
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


static void free_address(struct netbuf *address)
{
	mem_free(address->buf, address->maxlen);
	mem_free(address, sizeof(*address));
}

static void reject_invalid_ip_ports(void)
{
	const char *netids[] = {"udp", "udp6"};
	const char *hosts[] = {"127.0.0.1", "::1"};
	const char *invalid[] = {
		"1.256", "256.1", "-1.0", "1.-1", "foo.2", "1.foo",
		"1.2junk", "1.", ".1", "1. 2", "+1.2",
		"999999999999999999999999.1"
	};
	for (size_t family = 0; family < 2; ++family) {
		struct netconfig config = {0};
		config.nc_netid = (char *)netids[family];
		config.nc_semantics = NC_TPI_CLTS;
		char universal[128];
		for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) {
			snprintf(universal, sizeof(universal), "%s.%s",
				 hosts[family], invalid[i]);
			assert(uaddr2taddr(&config, universal) == NULL);
		}
		const char *valid[] = {"0.0", "1.2", "255.255"};
		for (size_t i = 0; i < sizeof(valid) / sizeof(valid[0]); ++i) {
			snprintf(universal, sizeof(universal), "%s.%s",
				 hosts[family], valid[i]);
			struct netbuf *address = uaddr2taddr(&config, universal);
			assert(address != NULL);
			char *roundtrip = taddr2uaddr(&config, address);
			assert(roundtrip != NULL && strcmp(roundtrip, universal) == 0);
			mem_free(roundtrip, 0);
			free_address(address);
		}
	}
}

static void unix_input_must_fit(void)
{
	struct netconfig config = local_config();
	char pathname[110];
	memset(pathname, 'a', sizeof(pathname));
	pathname[0] = '/';
	pathname[108] = '\0';
	assert(uaddr2taddr(&config, pathname) == NULL);
	assert(uaddr2taddr(&config, "relative.1.2") == NULL);
	pathname[107] = '\0';
	struct netbuf *address = uaddr2taddr(&config, pathname);
	assert(address != NULL);
	char *roundtrip = taddr2uaddr(&config, address);
	assert(roundtrip != NULL && strcmp(roundtrip, pathname) == 0);
	mem_free(roundtrip, 0);
	free_address(address);
}

int main(void)
{
	long_unix_address(59);
	long_unix_address(107);
	unix_address_respects_length();
	reject_invalid_ip_ports();
	unix_input_must_fit();
	return 0;
}
