#include "config.h"
#include <assert.h>
#include <errno.h>
#include <unistd.h>
#include <rpc/rpc.h>
#include "../vendor/ntirpc/src/svc_internal.h"
#include "../vendor/ntirpc/src/svc_xprt.h"

int main(void)
{
	svc_init_params params = {
		.flags = SVC_INIT_EPOLL | SVC_INIT_NOREG_XPRTS,
		.max_events = 8,
	};
	assert(svc_init(&params));
	int pair[2];
	assert(socketpair(AF_UNIX, SOCK_STREAM, 0, pair) == 0);
	SVCXPRT *x = svc_fd_ncreatef(pair[0], 0, 0, SVC_CREATE_FLAG_XPRT_NOREG);
	assert(x != NULL);
	int refs = x->xp_refcnt;
	SVCXPRT *found = svc_xprt_lookup(pair[0], NULL);
	assert(found == x);
	/* A successful lookup must hand the caller a locked duplex mutex. */
	assert(pthread_mutex_trylock(&REC_XPRT(x)->recv.lock.we.mtx) == EBUSY);
	rpc_dplx_rui(REC_XPRT(found));
	SVC_RELEASE(found, SVC_RELEASE_FLAG_NONE);
	for (int i = 0; i < 10; ++i) {
		assert(svc_validate_xprt_list(x));
		SVCXPRT stale = {.xp_fd = pair[0]};
		assert(!svc_validate_xprt_list(&stale));
	}
	assert(x->xp_refcnt == refs);
	SVC_DESTROY(x);
	SVC_RELEASE(x, SVC_RELEASE_FLAG_NONE);
	close(pair[0]);
	close(pair[1]);

	assert(svc_shutdown(SVC_SHUTDOWN_FLAG_NONE) == 0);
	return 0;
}
