import { createHead } from '@vueuse/head'
import { ViteSSG } from 'vite-ssg'
import App from './App.vue'
import './style.css'
import { routes } from './router'

export const createApp = ViteSSG(
  App,
  {
    routes,
    base: import.meta.env.BASE_URL
  },
  ({ app }) => {
    const head = createHead()
    app.use(head)
  }
)
