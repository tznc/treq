import { SWRConfig } from "swr";
import { Router } from "wouter";
import { useHashLocation } from "wouter/use-hash-location";
import { Dashboard } from "./components/Dashboard";
import { ToastProvider } from "./components/ui/toast";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { PrismThemeLoader } from "./components/PrismThemeLoader";
import { AppStoreEffects } from "./stores/AppStoreEffects";
import { defaultSWRConfig, SWRMutateScope } from "./lib/swr-cache";
import "./index.css";

function AppContent() {
  return (
    <div className="flex h-screen">
      <ErrorBoundary
        fallbackTitle="Dashboard crashed"
        onReset={() => {
          if (typeof window !== "undefined") {
            window.location.reload();
          }
        }}
      >
        <Dashboard />
      </ErrorBoundary>
    </div>
  );
}

function App() {
  return (
    <ErrorBoundary
      fallbackTitle="Application failed to initialize"
      onReset={() => {
        if (typeof window !== "undefined") {
          window.location.reload();
        }
      }}
    >
      <SWRConfig value={defaultSWRConfig}>
        <SWRMutateScope>
          <AppStoreEffects />
          <PrismThemeLoader />
          <ToastProvider>
            <Router hook={useHashLocation}>
              <AppContent />
            </Router>
          </ToastProvider>
        </SWRMutateScope>
      </SWRConfig>
    </ErrorBoundary>
  );
}

export default App;
