import type React from "react";
import clsx from "clsx";
import { useAuth } from "./AuthProvider";
import { APP_DEEP_LINK } from "../lib/constants";
import Link from "@docusaurus/Link";

type Props = {
  mobile?: boolean;
  className?: string;
};

export default function NavbarAuthLinks({
  mobile = false,
  className,
}: Props): React.ReactNode {
  const { user } = useAuth();
  const signedIn = !!user;

  const accountHref = signedIn ? "/dashboard" : "/sign-in";
  const accountLabel = signedIn ? "Dashboard" : "Sign in";

  const Wrapper: "li" | "div" = mobile ? "li" : "div";
  const wrapperClass = clsx(
    {
      navbar__item: !mobile,
      "menu__list-item": mobile,
    },
    className,
  );

  return (
    <>
      {/* The account link stays in the collapsed top bar (custom.css). */}
      <Wrapper className={clsx(wrapperClass, { "navbar-account": !mobile })}>
        <a
          href={accountHref}
          className="button button--secondary button--outline button--sm"
        >
          {accountLabel}
        </a>
      </Wrapper>
      <Wrapper className={wrapperClass}>
        {signedIn ? (
          <a href={APP_DEEP_LINK} className="button button--primary button--sm">
            Open App
          </a>
        ) : (
          <Link to="/download/" className="button button--primary button--sm">
            Download
          </Link>
        )}
      </Wrapper>
    </>
  );
}
