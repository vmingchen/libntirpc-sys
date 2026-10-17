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


static unsigned string_allocations;
static mem_1_size_t original_malloc;

static void *count_string_allocation(size_t size, const char *file,
				     int line, const char *function)
{
	++string_allocations;
	return original_malloc(size, file, line, function);
}

static void reject_string_allocation_overflow(void)
{
	uint32_t wire = htonl(UINT_MAX);
	XDR stream;
	char *value = NULL;
	xdrmem_ncreate(&stream, (char *)&wire, sizeof(wire), XDR_DECODE);
	original_malloc = __ntirpc_pkg_params.malloc_;
	__ntirpc_pkg_params.malloc_ = count_string_allocation;
	bool decoded = xdr_string(&stream, &value, UINT_MAX);
	__ntirpc_pkg_params.malloc_ = original_malloc;
	assert(!decoded);
	assert(value == NULL);
	assert(string_allocations == 0);

	/* The empty string still needs one byte for its terminating NUL. */
	wire = 0;
	xdrmem_ncreate(&stream, (char *)&wire, sizeof(wire), XDR_DECODE);
	assert(xdr_string(&stream, &value, UINT_MAX));
	assert(value != NULL && value[0] == '\0');
	XDR freeing = {.x_op = XDR_FREE};
	assert(xdr_string(&freeing, &value, UINT_MAX));
	assert(value == NULL);
}

int main(void)
{
	decode_unbounded_array(0);
	decode_unbounded_array(1);
	reject_invalid_array_size();
	reject_string_allocation_overflow();
	return 0;
}
