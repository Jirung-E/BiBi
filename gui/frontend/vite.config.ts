import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';
export default defineConfig({plugins:[sveltekit()],server:{host:'127.0.0.1',port:44881,strictPort:true,proxy:{'/api':'http://127.0.0.1:44880','/auth':'http://127.0.0.1:44880'}},test:{include:['src/**/*.test.ts']}});
