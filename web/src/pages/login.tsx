import type React from "react";
import BrowserOnly from "@docusaurus/BrowserOnly";

/** Permanent redirect from /login to /sign-in (preserves query string). */
export default function LoginRedirectPage(): React.ReactNode {
  return (
    <BrowserOnly>
      {() => {
        const {search, hash} = window.location;
        window.location.replace(`/sign-in${search}${hash}`);
        return null;
      }}
    </BrowserOnly>
  );
}
