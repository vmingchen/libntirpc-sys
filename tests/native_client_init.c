/* Exercise simultaneous first use without a running RPC service. */
#include <assert.h>
#include <pthread.h>
#include <rpc/rpc.h>
#include <rpc/nettype.h>
#include <netconfig.h>

#define THREADS 16
static pthread_barrier_t barrier;
static AUTH *auth[THREADS];

static void *first_call(void *argument)
{
	size_t index = (size_t)argument;
	pthread_barrier_wait(&barrier);
	auth[index] = authnone_ncreate();
	struct netconfig *config = __rpc_getconfip("udp");
	assert(config != NULL);
	freenetconfigent(config);
	/* Force the thread-specific netconfig error path. */
	assert(getnetconfigent("ntirpc-test-nonexistent-transport") == NULL);
	/* A bad nettype fails before any socket or request dispatch is needed. */
	assert(rpc_call("localhost", 1, 1, 1, NULL, NULL, NULL, NULL,
			"ntirpc-missing") == RPC_UNKNOWNPROTO);
	return NULL;
}

int main(void)
{
	pthread_t threads[THREADS];
	assert(pthread_barrier_init(&barrier, NULL, THREADS) == 0);
	for (size_t i = 0; i < THREADS; ++i)
		assert(pthread_create(&threads[i], NULL, first_call, (void *)i) == 0);
	for (size_t i = 0; i < THREADS; ++i) {
		assert(pthread_join(threads[i], NULL) == 0);
		assert(auth[i] != NULL && auth[i] == auth[0]);
	}
	assert(pthread_barrier_destroy(&barrier) == 0);
	return 0;
}
