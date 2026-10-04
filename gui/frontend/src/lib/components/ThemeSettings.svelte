<script lang="ts">
import {theme,setTheme,type ThemeChoice} from '../theme';
</script>

<div class="theme-setting">
 <label for="theme-choice">테마</label>
 <select id="theme-choice" value={$theme} onchange={event=>setTheme(event.currentTarget.value as ThemeChoice)}>
  <option value="system">시스템 설정 따르기</option>
  <option value="light">라이트</option>
  <option value="dark">다크</option>
 </select>
</div>

<style>
.theme-setting {display:grid;grid-template-columns:auto minmax(0,1fr);align-items:center;gap:1rem;margin-bottom:1rem;}
.theme-setting label {margin:0;}
.theme-setting select {min-width:0;}
</style>
