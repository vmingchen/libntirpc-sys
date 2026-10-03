/* SPDX-License-Identifier: LGPL-3.0-or-later */
/*
 * vim:noexpandtab:shiftwidth=8:tabstop=8:
 *
 * Copyright (C) 2025, IBM . All rights reserved.
 * Author: Deeraj Patil <deeraj.patil@ibm.com>
 *
 * This program is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 3 of the License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
 * 02110-1301 USA.  see <http://www.gnu.org/licenses/
 *
 * ---------------------------------------
 */

/**
 * @file rpc/tls.h
 * @brief Public TLS configuration structure and API for ntirpc.
 */

#ifndef TIRPC_TLS_H
#define TIRPC_TLS_H

#include <stdbool.h>
#include <time.h>

#ifdef USE_TLS

/* TLS configuration structure */
typedef struct tls_config {
	bool enabled;
	char *cert_file;
	char *key_file;
	char *ca_file;
	char *ciphers;
	char *min_version;
	time_t session_timeout; /* for future use for session key updates */
	bool ktls; /* Enable kernel TLS if available */
	bool debug;
} tls_config_t;

extern tls_config_t tls_config;
extern bool tls_init(tls_config_t *from_config);
extern void tls_cleanup(void);

#endif /* USE_TLS */

#endif /* TIRPC_TLS_H */
