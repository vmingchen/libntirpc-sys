"""Print this crate's native OUT_DIR from Cargo's JSON build messages."""
import json
import sys

with open(sys.argv[1]) as messages:
    for line in messages:
        message = json.loads(line)
        if (
            message.get("reason") == "build-script-executed"
            and "libntirpc-sys" in message["package_id"]
        ):
            print(message["out_dir"])
            break
    else:
        raise SystemExit("libntirpc-sys native build output not found")
