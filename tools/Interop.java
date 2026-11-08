// Cross-check against GeyserMC's own classes (Floodgate jar on the classpath, not part of this repo).
//   javac -cp floodgate-velocity.jar -d out tools/Interop.java
//   java -cp floodgate-velocity.jar:out Interop gen <key file> <vectors file>   (Java encrypts, Rust reads)
//   java -cp floodgate-velocity.jar:out Interop read <key file> <vectors file>  (Rust encrypts, Java reads)
// Vector lines: "case<TAB>label<TAB>encrypted<TAB>expected", expected = the BedrockData getters joined by '|'.
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import javax.crypto.SecretKey;
import org.geysermc.floodgate.crypto.AesCipher;
import org.geysermc.floodgate.crypto.AesKeyProducer;
import org.geysermc.floodgate.crypto.Base64Topping;
import org.geysermc.floodgate.util.BedrockData;
import org.geysermc.floodgate.util.LinkedPlayer;

public final class Interop {
    static String describe(BedrockData d) {
        String linked = d.getLinkedPlayer() == null ? "null"
                : d.getLinkedPlayer().getJavaUsername() + ";" + d.getLinkedPlayer().getJavaUniqueId()
                        + ";" + d.getLinkedPlayer().getBedrockId();
        return String.join("|", d.getVersion(), d.getUsername(), d.getXuid(),
                String.valueOf(d.getDeviceOs()), d.getLanguageCode(), String.valueOf(d.getUiProfile()),
                String.valueOf(d.getInputMode()), d.getIp(), linked, String.valueOf(d.isFromProxy()),
                String.valueOf(d.getSubscribeId()), d.getVerifyCode(), String.valueOf(d.getDataLength()));
    }

    public static void main(String[] args) throws Exception {
        AesCipher cipher = new AesCipher(new Base64Topping());
        if (args[0].equals("gen")) {
            SecretKey key = new AesKeyProducer().produce();
            Files.write(Path.of(args[1]), key.getEncoded());
            cipher.init(key);
            List<String> out = new ArrayList<>();
            BedrockData plain = BedrockData.of("1.21.130", "Steve Two", "2535400000000001", 7, "en_US",
                    1, 2, "203.0.113.9", null, true, 42, "verify-1");
            BedrockData linked = BedrockData.of("1.21.130", "LinkedOne", "2535400000000002", 1, "de_DE",
                    0, 1, "2001:db8::1", LinkedPlayer.of("JavaName",
                            UUID.fromString("11111111-2222-3333-4444-555555555555"),
                            UUID.fromString("00000000-0000-0000-0009-01eed05d1002")),
                    true, 7, "verify-2");
            for (Object[] c : new Object[][] {{"plain", plain}, {"linked", linked}}) {
                BedrockData d = (BedrockData) c[1];
                String enc = new String(cipher.encryptFromString(d.toString()), StandardCharsets.UTF_8);
                out.add("case\t" + c[0] + "\t" + enc + "\t" + describe(BedrockData.fromString(d.toString())));
            }
            String shortData = "a\0b\0c";
            out.add("case\tshort\t" + new String(cipher.encryptFromString(shortData), StandardCharsets.UTF_8)
                    + "\t" + describe(BedrockData.fromString(shortData)));
            Files.write(Path.of(args[2]), out);
        } else {
            cipher.init(new AesKeyProducer().produceFrom(Files.readAllBytes(Path.of(args[1]))));
            for (String line : Files.readAllLines(Path.of(args[2]))) {
                String[] p = line.split("\t");
                String dec = cipher.decryptToString(p[2].getBytes(StandardCharsets.UTF_8));
                System.out.println(p[1] + ": " + describe(BedrockData.fromString(dec)));
            }
        }
    }
}
