<script lang="ts">
import {markdown} from '../markdown';
let {text}:{text:string}=$props();
const html=$derived(markdown(text));
</script>
<div class="message-text markdown">{@html html}</div>
