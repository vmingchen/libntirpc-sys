#include <assert.h>
#include <limits.h>
#include <rpc/rpc.h>
#include <rpc/xdr_inline.h>

static bool element(XDR *xdrs, void *value)
{
	return xdr_u_int(xdrs, value);
}

static void decode_unbounded_array(unsigned count)
{
	uint32_t wire[] = {htonl(count), htonl(42)};
	XDR stream;
	char *values = NULL;
	u_int size = UINT_MAX;

	xdrmem_ncreate(&stream, (char *)wire, sizeof(wire), XDR_DECODE);
	assert(xdr_array(&stream, &values, &size, UINT_MAX,
			 sizeof(u_int), element));
	assert(size == count);
	if (count) {
		assert(((u_int *)values)[0] == 42);
		XDR freeing = {.x_op = XDR_FREE};
		assert(xdr_array(&freeing, &values, &size, UINT_MAX,
				 sizeof(u_int), element));
		assert(values == NULL);
	} else {
		assert(values == NULL);
	}
}

static void reject_invalid_array_size(void)
{
	uint32_t wire[] = {htonl(UINT_MAX)};
	XDR stream;
	char *values = NULL;
	u_int size = 0;

	xdrmem_ncreate(&stream, (char *)wire, sizeof(wire), XDR_DECODE);
	assert(!xdr_array(&stream, &values, &size, UINT_MAX,
			  sizeof(u_int), element));
	assert(values == NULL);
	assert(size == 0);
	xdrmem_ncreate(&stream, (char *)wire, sizeof(wire), XDR_DECODE);
	assert(!xdr_array(&stream, &values, &size, UINT_MAX, 0, element));
	assert(values == NULL);
}

int main(void)
{
	decode_unbounded_array(0);
	decode_unbounded_array(1);
	reject_invalid_array_size();
	return 0;
}
