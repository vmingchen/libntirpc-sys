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

	mutex_destroy(&sr.ev_lock);
	return 0;
}
