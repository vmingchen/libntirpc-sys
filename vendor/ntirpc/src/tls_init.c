#include "tls.h"
#include "tls_transport.h"


#ifdef USE_TLS
tls_config_t tls_config;

/* Initialize ntirpc TLS from configuration by keeping the local copy*/
bool tls_init(tls_config_t *from_config)
{
	memcpy(&tls_config, from_config, sizeof(tls_config_t));
	if (!tls_config.enabled) {
		LogDebugTLS(TLS_INIT, "TLS is disabled in configuration");
		return true;
	}

	LogDebugTLS(TLS_INIT, "Initializing TLS with cert=%s, key=%s, ca=%s",
		    tls_config.cert_file, tls_config.key_file,
		    tls_config.ca_file ? tls_config.ca_file : "none");

	/* Initialize TLS library */
	if (!xprt_tls_init(tls_config.cert_file, tls_config.key_file,
			   tls_config.ca_file, tls_config.ciphers,
			   tls_config.min_version, tls_config.ktls,
			   tls_config.debug)) {
		LogCritTLS(TLS_INIT, "Failed to initialize TLS");
		return false;
	}

	LogDebugTLS(TLS_INIT, "TLS initialized successfully");
	return true;
}

/* Cleanup ntiprc TLS config*/
void tls_cleanup(void)
{
	tls_config.cert_file = NULL;
	tls_config.key_file = NULL;
	tls_config.ca_file = NULL;
	tls_config.ciphers = NULL;
	tls_config.min_version = NULL;
	LogDebugTLS(TLS_INIT, "TLS resources cleaned up");
}

#endif /*  USE_TLS */
