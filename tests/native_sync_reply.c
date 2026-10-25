/* A synchronous request must retain its callback state across a slow decode.
 * Use the real request lookup/wait/release code with an in-process reply, so
 * this regression needs neither rpcbind nor a listening server.
 */
#include "config.h"
#include <assert.h>
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <rpc/rpc.h>
#include "../vendor/ntirpc/src/clnt_internal.h"

extern enum xprt_stat clnt_req_process_reply(SVCXPRT *, struct svc_req *);

static pthread_mutex_t control = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t changed = PTHREAD_COND_INITIALIZER;
static bool entered, resume_decode, returned, allow_cleanup;
static unsigned sends;
static pthread_t receiver;
static struct cx_data client;
static struct rpc_dplx_rec transport;
enum test_mode { SLOW_REPLY, SEND_ERROR, ZERO_TIMEOUT, NO_REPLY, DECODE_ERROR };
static enum test_mode mode;

static enum xprt_stat idle(SVCXPRT *xprt)
{
	return XPRT_IDLE;
}

static bool slow_decode(XDR *stream, void *slot)
{
	assert(mode == SLOW_REPLY || mode == DECODE_ERROR);
	if (mode == DECODE_ERROR)
		return false;
	pthread_mutex_lock(&control);
	entered = true;
	pthread_cond_broadcast(&changed);
	while (!resume_decode)
		pthread_cond_wait(&changed, &control);
	pthread_mutex_unlock(&control);
	*(int *)slot = 42;
	return true;
}

static void *reply(void *argument)
{
	struct clnt_req *cc = argument;
	XDR stream = {.x_op = XDR_DECODE};
	struct svc_req request = {.rq_xdrs = &stream};
	request.rq_msg.rm_xid = cc->cc_xid;
	request.rq_msg.rm_direction = REPLY;
	request.rq_msg.rm_reply.rp_stat = MSG_ACCEPTED;
	request.rq_msg.RPCM_ack.ar_stat = SUCCESS;
	request.rq_msg.RPCM_ack.ar_verf = _null_auth;
	clnt_req_process_reply(&transport.xprt, &request);
	return NULL;
}

static enum clnt_stat send_reply(struct clnt_req *cc)
{
	pthread_mutex_lock(&control);
	if (++sends == 1 && mode != NO_REPLY)
		assert(pthread_create(&receiver, NULL, reply, cc) == 0);
	pthread_mutex_unlock(&control);
	if (mode == SEND_ERROR || mode == ZERO_TIMEOUT) {
		/* The caller still owns cc_we.mtx. Wait until lookup has pinned
		 * the request but its decoder is queued behind that mutex.
		 */
		struct timespec deadline, now;
		clock_gettime(CLOCK_MONOTONIC, &deadline);
		deadline.tv_sec++;
		while (atomic_fetch_int32_t(&cc->cc_refcnt) == 1) {
			sched_yield();
			clock_gettime(CLOCK_MONOTONIC, &now);
			assert(now.tv_sec < deadline.tv_sec ||
			       (now.tv_sec == deadline.tv_sec &&
				now.tv_nsec < deadline.tv_nsec));
		}
		if (mode == SEND_ERROR)
			return RPC_CANTSEND;
	}
	return RPC_SUCCESS;
}

static void *call(void *unused)
{
	int result = 0;
	struct clnt_req *cc = mem_zalloc(sizeof(*cc));
	clnt_req_fill(cc, &client.cx_c, authnone_ncreate(), 1,
		      NULL, NULL, slow_decode, &result);
	struct timespec timeout = {.tv_nsec = 10000000}; /* 10ms/retry */
	if (mode == ZERO_TIMEOUT)
		timeout.tv_nsec = 0;
	assert(clnt_req_setup(cc, timeout) == RPC_SUCCESS);
	enum clnt_stat status = CLNT_CALL_WAIT(cc);

	if (mode == SLOW_REPLY) {
		pthread_mutex_lock(&control);
		returned = true;
		pthread_cond_broadcast(&changed);
		/* Keep the old implementation's dangling callback storage alive
		 * long enough for an orderly regression failure.
		 */
		while (!allow_cleanup)
			pthread_cond_wait(&changed, &control);
		pthread_mutex_unlock(&control);
		assert(status == RPC_SUCCESS);
		assert(result == 42);
	} else {
		enum clnt_stat expected = mode == SEND_ERROR ? RPC_CANTSEND :
			mode == NO_REPLY ? RPC_TIMEDOUT :
			mode == DECODE_ERROR ? RPC_CANTDECODERES : RPC_SUCCESS;
		assert(status == expected);
		assert(result == 0);
		if (mode != NO_REPLY)
			assert(pthread_join(receiver, NULL) == 0);
		assert(sends == (mode == NO_REPLY ? 3 : 1));
	}
	assert(atomic_fetch_int32_t(&cc->cc_refcnt) == 1);
	assert(!opr_rbtree_node_valid(&cc->cc_dplx));
	clnt_req_release(cc);
	return NULL;
}

int main(void)
{
	static struct clnt_ops operations = {.cl_call = send_reply};
	static struct xp_ops transport_ops = {.xp_stat = idle};
	clnt_data_init(&client);
	rpc_dplx_rec_init(&transport);
	client.cx_rec = &transport;
	client.cx_c.cl_ops = &operations;
	transport.xprt.xp_ops = &transport_ops;
	pthread_t caller;
	assert(pthread_create(&caller, NULL, call, NULL) == 0);

	pthread_mutex_lock(&control);
	while (!entered)
		pthread_cond_wait(&changed, &control);
	struct timespec deadline;
	clock_gettime(CLOCK_REALTIME, &deadline);
	deadline.tv_sec++; /* Longer than all three 10ms attempts combined. */
	int code = 0;
	while (!returned && code != ETIMEDOUT)
		code = pthread_cond_timedwait(&changed, &control, &deadline);
	if (returned) {
		fputs("synchronous call returned while its decoder was still active\n", stderr);
		_Exit(1);
	}
	assert(sends == 1); /* An active decoder must also prevent retries. */
	resume_decode = true;
	pthread_cond_broadcast(&changed);
	pthread_mutex_unlock(&control);
	assert(pthread_join(receiver, NULL) == 0);
	pthread_mutex_lock(&control);
	allow_cleanup = true;
	pthread_cond_broadcast(&changed);
	pthread_mutex_unlock(&control);
	assert(pthread_join(caller, NULL) == 0);
	assert(client.cx_c.cl_refcnt == 1);
	/* Error and zero-timeout exits must drain queued lookups without ever
	 * invoking their decoder. Ordinary no-reply timeouts still retry.
	 */
	for (mode = SEND_ERROR; mode <= DECODE_ERROR; ++mode) {
		sends = 0;
		call(NULL);
		assert(client.cx_c.cl_refcnt == 1);
	}
	rpc_dplx_rec_destroy(&transport);
	clnt_data_destroy(&client);
	return 0;
}
