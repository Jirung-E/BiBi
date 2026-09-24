<script lang="ts">
let {name}: {name:'sidebar'|'canvas'|'chat'|'usage'|'settings'|'plus'|'context'}=$props();
const paths={
 sidebar:'M8 4v16M4 4h16v16H4z',
 canvas:'M4 4h6v5H4zM14 15h6v5h-6zM14 4h6v5h-6zM7 9v8h7M10 6.5h4',
 chat:'M4 4h16v12H9l-5 4zM8 8h8M8 12h5',
 usage:'M4 20V10M10 20V4M16 20v-8M22 20H2',
 settings:'M9 3h6l1 4 4 1v6l-4 1-1 4H9l-1-4-4-1V8l4-1zM15 11a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
 plus:'M12 5v14M5 12h14',
 context:'M16 4v16M4 4h16v16H4zM8 8h4M8 12h4'
};
</script>
<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
