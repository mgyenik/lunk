import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'

// Dark mode: auto-detect from system, allow manual override via localStorage
if (
  localStorage.theme === 'dark' ||
  (!localStorage.theme && window.matchMedia('(prefers-color-scheme: dark)').matches)
) {
  document.documentElement.classList.add('dark');
}

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
