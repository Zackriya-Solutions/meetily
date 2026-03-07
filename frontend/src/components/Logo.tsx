import React from "react";
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from "./ui/dialog";
import { VisuallyHidden } from "./ui/visually-hidden";
import { About } from "./About";
import { ClearMinutesIcon, ClearMinutesWordmark } from "./ClearMinutesLogo";

interface LogoProps {
    isCollapsed: boolean;
}

const Logo = React.forwardRef<HTMLButtonElement, LogoProps>(({ isCollapsed }, ref) => {
  return (
    <Dialog aria-describedby={undefined}>
      {isCollapsed ? (
        <DialogTrigger asChild>
          <button ref={ref} className="flex items-center justify-start mb-2 cursor-pointer bg-transparent border-none p-0 hover:opacity-80 transition-opacity">
            <ClearMinutesIcon size={36} />
          </button>
        </DialogTrigger>
      ) : (
        <DialogTrigger asChild>
          <button ref={ref} className="flex items-center mb-2 cursor-pointer bg-transparent border-none p-0 hover:opacity-80 transition-opacity w-full">
            <ClearMinutesWordmark iconSize={28} />
          </button>
        </DialogTrigger>
      )}
      <DialogContent>
        <VisuallyHidden>
          <DialogTitle>About Clearminutes</DialogTitle>
        </VisuallyHidden>
        <About />
      </DialogContent>
    </Dialog>
  );
});

Logo.displayName = "Logo";

export default Logo;