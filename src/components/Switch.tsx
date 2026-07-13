import * as RadixSwitch from "@radix-ui/react-switch";

type Props = {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
};

export function Switch({ checked, onCheckedChange }: Props) {
  return (
    <RadixSwitch.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      className="relative h-5 w-9 rounded-full border border-border bg-slate-200 data-[state=checked]:bg-accent"
    >
      <RadixSwitch.Thumb className="block h-4 w-4 translate-x-0.5 rounded-full bg-white shadow-soft transition-transform data-[state=checked]:translate-x-4" />
    </RadixSwitch.Root>
  );
}
