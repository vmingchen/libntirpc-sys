#include <assert.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netdb.h>
#include <rpc/rpc.h>
#include <misc/queue.h>

struct broadif {
	int index;
	struct sockaddr_storage broadaddr;
	TAILQ_ENTRY(broadif) link;
};
typedef TAILQ_HEAD(, broadif) broadlist_t;
extern int __rpc_getbroadifs(int, int, int, broadlist_t *);
extern void __rpc_freebroadifs(broadlist_t *);

static struct ifaddrs interface;
static struct sockaddr_in6 address = {.sin6_family = AF_INET6};
static struct addrinfo resolved = {.ai_addr = (struct sockaddr *)&address};
static bool fail_resolution;
static bool fail_conversion = true;
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
	if (fail_conversion)
		return 0;
	assert(family == AF_INET6);
	const unsigned char multicast[16] = {0xff, 0x02, [15] = 0x02};
	memcpy(destination, multicast, sizeof(multicast));
	return 1;
}

static void *count_alloc(size_t size, const char *file, int line, const char *func)
{
	++allocations;
	return malloc(size);
}

static void *poison_alloc(size_t size, const char *file, int line, const char *func)
{
	void *result = count_alloc(size, file, line, func);
	memset(result, 0xa5, size);
	return result;
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

	/* Supply exactly a sockaddr_in's readable bytes. ASan catches the old
	 * 128-byte overread; zero-tail checks cover destination padding in normal
	 * builds. Poison destination allocations to expose uninitialized bytes.
	 */
	struct sockaddr_in *broadcast = malloc(sizeof(*broadcast));
	*broadcast = (struct sockaddr_in){
		.sin_family = AF_INET, .sin_addr.s_addr = htonl(0xc00002ff)
	};
	struct sockaddr_in local = {.sin_family = AF_INET};
	struct sockaddr_in service = {.sin_family = AF_INET, .sin_port = htons(111)};
	interface.ifa_addr = (struct sockaddr *)&local;
	interface.ifa_broadaddr = (struct sockaddr *)broadcast;
	interface.ifa_flags = IFF_UP | IFF_BROADCAST;
	resolved.ai_addr = (struct sockaddr *)&service;
	__ntirpc_pkg_params.malloc_ = poison_alloc;
	assert(__rpc_getbroadifs(AF_INET, IPPROTO_UDP, SOCK_DGRAM, &list) == 1);
	struct broadif *entry = TAILQ_FIRST(&list);
	struct sockaddr_in expected = *broadcast;
	expected.sin_port = htons(111);
	assert(memcmp(&entry->broadaddr, &expected, sizeof(expected)) == 0);
	for (size_t i = sizeof(expected); i < sizeof(entry->broadaddr); ++i)
		assert(((unsigned char *)&entry->broadaddr)[i] == 0);
	__rpc_freebroadifs(&list);
	TAILQ_INIT(&list);
	free(broadcast);

	/* IPv6 construction must also initialize flowinfo and storage padding. */
	interface.ifa_addr = (struct sockaddr *)&address;
	interface.ifa_flags = IFF_UP | IFF_MULTICAST;
	resolved.ai_addr = (struct sockaddr *)&address;
	address.sin6_port = htons(111);
	fail_conversion = false;
	assert(__rpc_getbroadifs(AF_INET6, IPPROTO_UDP, SOCK_DGRAM, &list) == 1);
	entry = TAILQ_FIRST(&list);
	struct sockaddr_in6 *multicast = (struct sockaddr_in6 *)&entry->broadaddr;
	assert(multicast->sin6_family == AF_INET6);
	assert(multicast->sin6_port == htons(111));
	assert(multicast->sin6_flowinfo == 0);
	assert(multicast->sin6_addr.s6_addr[0] == 0xff);
	assert(multicast->sin6_addr.s6_addr[15] == 0x02);
	assert(multicast->sin6_scope_id == entry->index);
	for (size_t i = sizeof(*multicast); i < sizeof(entry->broadaddr); ++i)
		assert(((unsigned char *)&entry->broadaddr)[i] == 0);
	__rpc_freebroadifs(&list);
	assert(allocations == releases);
	return 0;
}
