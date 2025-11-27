import type { RouteRecordRaw } from 'vue-router'
import HomeView from './views/HomeView.vue'

export const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'home-zh',
    component: HomeView,
    meta: { lang: 'zh' }
  },
  {
    path: '/en/',
    name: 'home-en',
    component: HomeView,
    meta: { lang: 'en' }
  },
  {
    path: '/zh/',
    redirect: '/'
  },
  {
    path: '/:pathMatch(.*)*',
    redirect: '/'
  }
]
