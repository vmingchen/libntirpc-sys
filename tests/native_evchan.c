/* Include the implementation to exercise private channel teardown without
 * duplicating its layout or starting worker threads. Other objects come from
 * the same bundled archive used by consumers. */
#include "../vendor/ntirpc/src/svc_rqst.c"

int main(void)
{
	struct svc_rqst_rec sr = {0};
	svc_rqst_rec_init(&sr);
	sr.ev_u.epoll.epoll_fd = epoll_create1(EPOLL_CLOEXEC);
	assert(sr.ev_u.epoll.epoll_fd >= 0);
	sr.ev_u.epoll.max_events = 4;
	sr.ev_u.epoll.events = mem_alloc(4 * sizeof(struct epoll_event));
	svc_rqst_rec_destroy(&sr);
	assert(sr.ev_u.epoll.events == NULL);
	assert(sr.ev_u.epoll.max_events == 0);
	assert(sr.ev_u.epoll.epoll_fd == -1);
	/* Reusing the descriptor must survive a second teardown. */
	int fd = open("/dev/null", O_RDONLY);
	assert(fd >= 0);
	svc_rqst_rec_destroy(&sr);
	assert(fcntl(fd, F_GETFD) >= 0);
	close(fd);

	/* Descriptor zero is valid, including when stdin was closed at startup. */
	fd = epoll_create1(EPOLL_CLOEXEC);
	assert(fd >= 0);
	assert(dup2(fd, 0) == 0);
	if (fd != 0)
		close(fd);
	sr.ev_u.epoll.epoll_fd = 0;
	svc_rqst_rec_destroy(&sr);
	assert(fcntl(0, F_GETFD) == -1 && errno == EBADF);

	/* Re-registering on the same channel must balance the lookup reference. */
	svc_rqst_init(2);
	struct svc_rqst_rec *channel = &svc_rqst_set.srr[1];
	channel->ev_refcnt = 1;
	struct rpc_dplx_rec rec = {0};
	rpc_dplx_rec_init(&rec);
	rec.ev_p = channel;
	for (int i = 0; i < 10; ++i)
		assert(svc_rqst_evchan_reg(1, &rec.xprt, 0) == 0);
	assert(channel->ev_refcnt == 1);
	rpc_dplx_rec_destroy(&rec);
	mutex_destroy(&sr.ev_lock);
	for (int i = 0; i < 2; ++i)
		mutex_destroy(&svc_rqst_set.srr[i].ev_lock);
	mem_free(svc_rqst_set.srr, 2 * sizeof(struct svc_rqst_rec));
	return 0;
}
