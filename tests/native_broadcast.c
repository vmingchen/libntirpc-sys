#include <assert.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netdb.h>
#include <rpc/rpc.h>
#include <misc/queue.h>

struct broadif;
typedef TAILQ_HEAD(, broadif) broadlist_t;
extern int __rpc_getbroadifs(int, int, int, broadlist_t *);

static struct ifaddrs interface;
static struct sockaddr_in6 address = {.sin6_family = AF_INET6};
static struct addrinfo resolved = {.ai_addr = (struct sockaddr *)&address};
static bool fail_resolution;
static unsigned lists_freed, addresses_freed, allocations, releases;

/* Deterministic interfaces: no dependency on host configuration or DNS. */
int getifaddrs(struct ifaddrs **result)
{
	*result = &interface;
	return 0;
}

void freeifaddrs(struct ifaddrs *list)
{
	assert(list == &interface);
	++lists_freed;
}

int getaddrinfo(const char *host, const char *service,
		const struct addrinfo *hints, struct addrinfo **result)
{
	if (fail_resolution)
		return EAI_FAIL;
	*result = &resolved;
	return 0;
}

void freeaddrinfo(struct addrinfo *result)
{
	assert(result == &resolved);
	++addresses_freed;
}

int inet_pton(int family, const char *text, void *destination)
{
	return 0;
}

static void *count_alloc(size_t size, const char *file, int line, const char *func)
{
	++allocations;
	return malloc(size);
}

static void count_free(void *ptr, size_t size, const char *file, int line,
		       const char *func)
{
	++releases;
	free(ptr);
}

int main(void)
{
	broadlist_t list;
	TAILQ_INIT(&list);
	/* An interface may have no L3 address. */
	assert(__rpc_getbroadifs(AF_INET6, IPPROTO_UDP, SOCK_DGRAM, &list) == 0);
	assert(lists_freed == 1 && addresses_freed == 1);
	fail_resolution = true;
	assert(__rpc_getbroadifs(AF_INET6, IPPROTO_UDP, SOCK_DGRAM, &list) == 0);
	assert(lists_freed == 2 && addresses_freed == 1);
	fail_resolution = false;
	interface.ifa_addr = (struct sockaddr *)&address;
	interface.ifa_flags = IFF_UP | IFF_MULTICAST;
	interface.ifa_name = "lo";
	__ntirpc_pkg_params.malloc_ = count_alloc;
	__ntirpc_pkg_params.free_size_ = count_free;
	assert(__rpc_getbroadifs(AF_INET6, IPPROTO_UDP, SOCK_DGRAM, &list) == 0);
	assert(allocations == 1 && releases == 1);
	assert(lists_freed == 3 && addresses_freed == 2);
	assert(TAILQ_EMPTY(&list));
	return 0;
}
