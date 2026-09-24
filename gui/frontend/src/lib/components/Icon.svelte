<script lang="ts">
let {name}: {name:'sidebar'|'canvas'|'chat'|'usage'|'settings'|'plus'|'minus'|'close'|'open'|'help'|'context'|'history'}=$props();
const paths={
 history:'M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5M12 7v5l3 2',
 sidebar:'M9 4v16M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2',
 canvas:'M4 4h6v5H4zM14 15h6v5h-6zM14 4h6v5h-6zM7 9v8h7M10 6.5h4',
 chat:'M4 4h16v12H9l-5 4zM8 8h8M8 12h5',
 usage:'M4 20V10M10 20V4M16 20v-8M22 20H2',
 settings:'M4 7h16M4 17h16M8 4v6M16 14v6',
 plus:'M12 5v14M5 12h14',
 minus:'M5 12h14',
 close:'m6 6 12 12M6 18 18 6',
 open:'M8 5H5v14h14v-3M13 5h6v6M19 5 10 14',
 help:'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18M9.5 9a2.5 2.5 0 1 1 4 2c-1 .7-1.5 1-1.5 2M12 16h.01',
 context:'M16 4v16M4 4h16v16H4zM8 8h4M8 12h4'
};
</script>
<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
