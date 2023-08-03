import * as wasm from "./pkg/rs_lib.js"

(async function main() {
    process.stdout.write(await wasm.__provider_main(process.argv[2], process.argv[3] || null))
})();
