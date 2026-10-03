import {prepareDependencies} from './desktop-tools.mjs';

// Keep just install a clean lockfile install, and let subsequent Tauri builds
// recognize it instead of running npm ci for a second time.
prepareDependencies({force: true});
