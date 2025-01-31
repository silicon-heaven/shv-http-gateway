
import Login from './Login.vue'
import Main from './Main.vue'

import { createRouter, createWebHashHistory } from "vue-router";

const routes = [
  { path: "/", "redirect": "/main" },
  {
    path: "/main",
    component: Main,
    meta: { requiresAuth: true }, // Protected route
  },
  { path: "/login", component: Login },
];

const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

// Navigation Guard
router.beforeEach((to, _from, next) => {
  const isAuthenticated = !!localStorage.getItem("session_id"); // Check token

  if (to.meta.requiresAuth && !isAuthenticated) {
    next("/login"); // Redirect to login if not authenticated
  } else if (to.matched.length == 0) {
    // Non-existing routes to default
    next("/");
  } else {
    // Allow access
    next();
  }
});

export default router;
