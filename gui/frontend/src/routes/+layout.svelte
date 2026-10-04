<script lang="ts">
import '../app.css';
import {onMount} from 'svelte';
import {initTheme} from '$lib/theme';
onMount(initTheme);
let { children } = $props();
</script>
{@render children()}
