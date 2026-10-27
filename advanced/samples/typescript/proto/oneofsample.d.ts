// The protobufjs interfaces and enums of descriptors/oneofsample/v1/oneof.proto, kept by hand. The
// generated code only uses them as types.
export namespace oneofsample {
  namespace v1 {
    enum Shade {
      SHADE_UNSPECIFIED = 0,
      SHADE_LIGHT = 1,
      SHADE_DARK = 2,
    }
    interface IPaint {
      tone?: oneofsample.v1.Shade | null;
      name?: string | null;
      pick?: "tone" | "name";
    }
  }
}
